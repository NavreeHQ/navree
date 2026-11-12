//! A survey of an app's page routes as a native client gets them: the
//! `native:coverage` command of the app, which `navree native:coverage` runs.
//!
//! Every route that can be a page is asked through the app's own router, in
//! process, with no socket, browser or device: once as the native client, for
//! its answer and its grade, and once as a browser, for the size of the HTML
//! beside it. A route with parameters needs example values, which the
//! [`Plan`] carries. A route that cannot be asked is listed as skipped, with
//! the reason; a route that is not a page by its nature (a data route, an event
//! stream, the toolkit's own files) is counted, not listed.

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use http::header::{self, HeaderName, HeaderValue};
use http::{Method, Request};
use navree_http::RouteMeta;
use navree_http::body::{self, Body};
use navree_http::response::Response;
use navree_kernel::{AppContext, BoxError};
use navree_native::{CLIENT_HEADER, CONTENT_TYPE as NATIVE_TYPE, Client, Envelope, NATIVE_HEADER};
use serde::{Deserialize, Serialize};
use tower::{Service, ServiceExt};

use super::Answered;

/// The most of an answer the survey reads.
const READ_AT_MOST: usize = 8 * 1024 * 1024;

/// The longest a route may take to answer.
const ASK_AT_MOST: Duration = Duration::from_secs(20);

/// What the survey is told by the author of the app: the example values of
/// route parameters, more addresses to ask, routes to leave out, and the
/// headers of a signed-in person. `navree native:coverage` reads it from
/// `native.toml` and hands it to the app as JSON.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Plan {
    /// A value for each parameter name, for every route that has it.
    pub values: BTreeMap<String, String>,
    /// Values for one route's parameters (by the route's pattern), which
    /// rank above `values`.
    pub routes: BTreeMap<String, BTreeMap<String, String>>,
    /// More addresses to ask, with their queries.
    pub also: Vec<String>,
    /// Routes to leave out on purpose: patterns in which `*` matches any text.
    pub skip: Vec<String>,
    /// Headers sent with every request (a session cookie).
    pub headers: BTreeMap<String, String>,
}

/// Why a pattern could not be turned into an address.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Fill {
    /// These parameters have no value.
    Missing(Vec<String>),
    /// The value of a parameter would leave its segment.
    Unsafe { name: String, value: String },
}

impl Plan {
    /// The value for parameter `name` of the route `pattern`.
    fn value(&self, pattern: &str, name: &str) -> Option<&str> {
        self.routes
            .get(pattern)
            .and_then(|own| own.get(name))
            .or_else(|| self.values.get(name))
            .map(String::as_str)
    }

    /// The address of the route `pattern`, its parameters replaced.
    pub(crate) fn fill(&self, pattern: &str) -> Result<String, Fill> {
        let mut out = String::new();
        let mut missing = Vec::new();
        for segment in pattern.split('/').skip(1) {
            out.push('/');
            let Some(inner) = segment
                .strip_prefix('{')
                .and_then(|rest| rest.strip_suffix('}'))
            else {
                out.push_str(segment);
                continue;
            };
            let (rest, name) = match inner.strip_prefix('*') {
                Some(name) => (true, name),
                None => (false, inner),
            };
            let Some(value) = self.value(pattern, name) else {
                missing.push(name.to_owned());
                continue;
            };
            let bad = value.is_empty()
                || value
                    .chars()
                    .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '?' | '#' | '\\'))
                || (!rest && value.contains('/'))
                || value.split('/').any(|part| part == "..");
            if bad {
                return Err(Fill::Unsafe {
                    name: name.to_owned(),
                    value: value.to_owned(),
                });
            }
            out.push_str(value);
        }
        if missing.is_empty() {
            if out.is_empty() {
                out.push('/');
            }
            Ok(out)
        } else {
            Err(Fill::Missing(missing))
        }
    }

    /// The pattern of `skip` that leaves out the route at `pattern`, if any.
    fn skipped_by(&self, pattern: &str) -> Option<&str> {
        self.skip
            .iter()
            .map(String::as_str)
            .find(|skip| glob(skip, pattern))
    }
}

/// Whether `text` matches `pattern`, in which `*` matches any text.
fn glob(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    let [first, rest @ ..] = parts.as_slice() else {
        return false;
    };
    let Some(mut left) = text.strip_prefix(first) else {
        return false;
    };
    let Some((last, middle)) = rest.split_last() else {
        return left.is_empty();
    };
    for part in middle {
        match left.find(part) {
            Some(at) => left = &left[at + part.len()..],
            None => return false,
        }
    }
    left.ends_with(last)
}

/// What the survey does with one route.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Pick {
    /// Ask this address.
    Ask(String),
    /// It is not a page by its nature; counted, under this reason.
    NotAPage(&'static str),
    /// It could be a page and was not asked; listed, with the reason.
    Skip(String),
}

/// Whether the route at `meta` is asked, and at which address.
pub(crate) fn pick(meta: &RouteMeta, api_prefix: &str, plan: &Plan) -> Pick {
    use navree_http::RouteKind;

    let path = meta.full_path.as_str();
    if meta.method != "GET" {
        return Pick::NotAPage("not a GET");
    }
    if path.starts_with("/_navree/") || path.starts_with("/.well-known/") {
        return Pick::NotAPage("the toolkit's and the framework's own files");
    }
    if meta.kind == RouteKind::Json {
        return Pick::NotAPage("data routes");
    }
    let prefix = api_prefix.trim_end_matches('/');
    let under_api = !prefix.is_empty()
        && (path == prefix
            || path
                .strip_prefix(prefix)
                .is_some_and(|rest| rest.starts_with('/')));
    if under_api && meta.kind != RouteKind::Page {
        return Pick::NotAPage("routes under the API prefix");
    }
    if let Some(skip) = plan.skipped_by(path) {
        return Pick::Skip(format!("left out by `{skip}` in the plan"));
    }
    match plan.fill(path) {
        Ok(address) => Pick::Ask(address),
        Err(Fill::Missing(names)) => Pick::Skip(format!(
            "needs a value for {}",
            names
                .iter()
                .map(|name| format!("{{{name}}}"))
                .collect::<Vec<_>>()
                .join(", ")
        )),
        Err(Fill::Unsafe { name, value }) => Pick::Skip(format!(
            "the value `{value}` of {{{name}}} would leave its segment"
        )),
    }
}

/// One page, asked and graded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Row {
    /// The route's pattern (`/orders/{id}`), or the address for an extra one.
    pub route: String,
    /// The address that was asked.
    pub path: String,
    /// `idiom`, `layout` or `web`.
    pub grade: String,
    /// Whether the client got native parts (a screen) and not its web view.
    pub native: bool,
    /// What kept a page in the web view.
    pub why: Vec<String>,
    /// What keeps a native screen from being idiom, in words.
    pub missing: Vec<String>,
    /// Decoration left out of the screen.
    pub dropped: Vec<String>,
    /// The size of the page as HTML, in bytes.
    pub html_bytes: usize,
    /// The size of the native answer, in bytes.
    pub envelope_bytes: usize,
}

/// What asking one address came to.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Asked {
    /// A page, graded.
    Page(Box<Row>),
    /// Not a page: counted, under this reason.
    NotAPage(String),
    /// Could not be read as a page: listed, with the reason.
    Skipped(String),
}

/// The headers of a request: the plan's, and the client's.
fn headers_of(plan: &Plan) -> Result<Vec<(HeaderName, HeaderValue)>, String> {
    plan.headers
        .iter()
        .map(|(name, value)| {
            let header = HeaderName::try_from(name.as_str())
                .map_err(|_| format!("`{name}` is not a header name"))?;
            let value = HeaderValue::from_str(value)
                .map_err(|_| format!("the value of the header `{name}` is not valid"))?;
            Ok((header, value))
        })
        .collect()
}

fn request(
    path: &str,
    client: Option<&Client>,
    extra: &[(HeaderName, HeaderValue)],
) -> Result<Request<Body>, BoxError> {
    let mut request = Request::builder()
        .method(Method::GET)
        .uri(path)
        .body(Body::empty())?;
    let headers = request.headers_mut();
    match client {
        Some(client) => {
            headers.insert(NATIVE_HEADER, HeaderValue::from_static("1"));
            headers.insert(CLIENT_HEADER, HeaderValue::from_str(&client.to_header())?);
        }
        None => {
            headers.insert(
                header::ACCEPT,
                HeaderValue::from_static("text/html,application/xhtml+xml"),
            );
        }
    }
    for (name, value) in extra {
        headers.insert(name.clone(), value.clone());
    }
    let peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 49152);
    request
        .extensions_mut()
        .insert(navree_http::serve::ConnectInfo(peer));
    Ok(request)
}

/// Sends one request through `router`, waiting at most [`ASK_AT_MOST`].
async fn send<S>(router: &S, request: Request<Body>) -> Result<Response, String>
where
    S: Service<Request<Body>, Response = Response, Error = Infallible> + Clone,
{
    let answer = router.clone().oneshot(request);
    match tokio::time::timeout(ASK_AT_MOST, answer).await {
        Ok(Ok(response)) => Ok(response),
        Ok(Err(never)) => match never {},
        Err(_) => Err(format!("no answer in {} seconds", ASK_AT_MOST.as_secs())),
    }
}

/// The reason to give for an answer that is not a page: a person who is signed
/// out or not allowed, or a redirect.
fn refusal(status: u16, location: Option<&str>) -> String {
    let signed_out = || {
        format!(
            "answered {status}: signed out, or not allowed; give a session in `[coverage.headers]`"
        )
    };
    match location {
        Some(at) => {
            let lower = at.to_lowercase();
            if ["sign-in", "signin", "sign_in", "login", "log-in"]
                .iter()
                .any(|word| lower.contains(word))
            {
                format!(
                    "answered {status} to {at}: signed out; give a session in `[coverage.headers]`"
                )
            } else {
                format!(
                    "answered {status} to {at}: a redirect is not a page; add the address it leads to under `also`"
                )
            }
        }
        None => signed_out(),
    }
}

/// Asks `path` of `router` as `client`, and as a browser for the HTML's size.
pub(crate) async fn ask<S>(
    router: &S,
    route: &str,
    path: &str,
    client: &Client,
    extra: &[(HeaderName, HeaderValue)],
) -> Asked
where
    S: Service<Request<Body>, Response = Response, Error = Infallible> + Clone,
{
    let native = match request(path, Some(client), extra) {
        Ok(request) => request,
        Err(error) => return Asked::Skipped(format!("cannot ask: {error}")),
    };
    let response = match send(router, native).await {
        Ok(response) => response,
        Err(reason) => return Asked::Skipped(reason),
    };
    let (parts, body) = response.into_parts();
    let status = parts.status.as_u16();
    let text = |name: header::HeaderName| {
        parts
            .headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    };
    let kind = text(header::CONTENT_TYPE)
        .unwrap_or_default()
        .to_lowercase();
    let location = text(header::LOCATION);
    match status {
        300..=399 => return Asked::Skipped(refusal(status, location.as_deref())),
        401 | 403 => return Asked::Skipped(refusal(status, None)),
        404 => {
            return Asked::Skipped(
                "answered 404: give an address that exists, in `[coverage.params]`".to_owned(),
            );
        }
        200..=299 => {}
        other => return Asked::Skipped(format!("answered {other}")),
    }
    if kind.starts_with("text/event-stream") {
        return Asked::NotAPage("event streams".to_owned());
    }
    if !kind.starts_with(NATIVE_TYPE) {
        let name = kind.split(';').next().unwrap_or("").trim();
        return Asked::NotAPage(if name.is_empty() {
            "answers that are not pages".to_owned()
        } else {
            format!("routes that answer {name}")
        });
    }
    let bytes = match body::to_bytes(body, READ_AT_MOST).await {
        Ok(bytes) => bytes,
        Err(error) => return Asked::Skipped(format!("the answer could not be read: {error}")),
    };
    let envelope: Envelope = match serde_json::from_slice(&bytes) {
        Ok(envelope) => envelope,
        Err(error) => {
            return Asked::Skipped(format!(
                "the answer is not an envelope a client reads: {error}"
            ));
        }
    };
    // An answer with no screen and no web view is a reply's ops: a redirect, say.
    if envelope.screen.is_none() && envelope.web.is_none() {
        let leads_to = envelope.ops.iter().find_map(|op| match op {
            navree_native::Op::Navigate { href } => Some(href.as_str()),
            _ => None,
        });
        return Asked::Skipped(match leads_to {
            Some(href) => format!(
                "answered a redirect to {href}: not a page; add the address it leads to under `also`"
            ),
            None => "answered operations, not a screen".to_owned(),
        });
    }
    let answered = Answered::of(&envelope);
    let html_bytes = html_size(router, path, extra).await;
    Asked::Page(Box::new(Row {
        route: route.to_owned(),
        path: path.to_owned(),
        grade: answered.grade.as_str().to_owned(),
        native: answered.native,
        why: answered.why,
        missing: answered.missing,
        dropped: answered.dropped,
        html_bytes,
        envelope_bytes: bytes.len(),
    }))
}

/// How many bytes `path` is as a browser gets it; 0 when it is not HTML.
async fn html_size<S>(router: &S, path: &str, extra: &[(HeaderName, HeaderValue)]) -> usize
where
    S: Service<Request<Body>, Response = Response, Error = Infallible> + Clone,
{
    let Ok(request) = request(path, None, extra) else {
        return 0;
    };
    let Ok(response) = send(router, request).await else {
        return 0;
    };
    let (parts, body) = response.into_parts();
    let html = parts
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|kind| kind.starts_with("text/html"));
    if !html || !parts.status.is_success() {
        return 0;
    }
    body::to_bytes(body, READ_AT_MOST)
        .await
        .map_or(0, |bytes| bytes.len())
}

/// A route that could not be asked, and why.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Skipped {
    /// The route's pattern.
    pub route: String,
    /// The address that was tried, when there was one.
    pub path: Option<String>,
    /// Why it was not read.
    pub reason: String,
}

/// How many routes are not pages, by reason.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct NotPages {
    /// What they are.
    pub reason: String,
    /// How many.
    pub count: usize,
}

/// The report of a survey.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub(crate) struct Report {
    /// The format's name.
    pub format: &'static str,
    /// Its version.
    pub version: u32,
    /// The platform asked as.
    pub platform: String,
    /// The form asked as.
    pub form: String,
    /// Every page, graded, in the order of the route table.
    pub pages: Vec<Row>,
    /// Routes that might be pages and could not be read.
    pub skipped: Vec<Skipped>,
    /// Routes that are not pages, counted.
    pub not_pages: Vec<NotPages>,
}

impl Report {
    fn new(client: &Client) -> Self {
        Self {
            format: "navree-native-coverage",
            version: 1,
            platform: client.platform.as_str().to_owned(),
            form: client.form.as_str().to_owned(),
            ..Self::default()
        }
    }

    fn not_a_page(&mut self, reason: &str) {
        match self.not_pages.iter_mut().find(|n| n.reason == reason) {
            Some(known) => known.count += 1,
            None => self.not_pages.push(NotPages {
                reason: reason.to_owned(),
                count: 1,
            }),
        }
    }
}

/// Asks every page route of `routes` (and the plan's extra addresses) of
/// `router`.
pub(crate) async fn survey<S>(
    router: &S,
    routes: &[RouteMeta],
    api_prefix: &str,
    plan: &Plan,
    client: &Client,
) -> Result<Report, String>
where
    S: Service<Request<Body>, Response = Response, Error = Infallible> + Clone,
{
    let extra = headers_of(plan)?;
    let mut report = Report::new(client);
    let mut seen: Vec<String> = Vec::new();
    let mut asks: Vec<(String, String)> = Vec::new();
    for meta in routes {
        match pick(meta, api_prefix, plan) {
            Pick::Ask(path) => {
                if !seen.contains(&path) {
                    seen.push(path.clone());
                    asks.push((meta.full_path.clone(), path));
                }
            }
            Pick::NotAPage(reason) => report.not_a_page(reason),
            Pick::Skip(reason) => report.skipped.push(Skipped {
                route: meta.full_path.clone(),
                path: None,
                reason,
            }),
        }
    }
    for path in &plan.also {
        if !path.starts_with('/') || path.starts_with("//") {
            return Err(format!(
                "`also` takes addresses of the app, such as /orders?status=open, not `{path}`"
            ));
        }
        if !seen.contains(path) {
            seen.push(path.clone());
            asks.push((path.clone(), path.clone()));
        }
    }
    for (route, path) in asks {
        match ask(router, &route, &path, client, &extra).await {
            Asked::Page(row) => report.pages.push(*row),
            Asked::NotAPage(reason) => report.not_a_page(&reason),
            Asked::Skipped(reason) => report.skipped.push(Skipped {
                route,
                path: Some(path),
                reason,
            }),
        }
    }
    Ok(report)
}

/// The value of `--name <value>` or `--name=<value>` among `args`.
fn flag(args: &[String], name: &str) -> Result<Option<String>, BoxError> {
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == name {
            return args
                .next()
                .cloned()
                .map(Some)
                .ok_or_else(|| format!("{name} needs a value").into());
        }
        if let Some(value) = arg
            .strip_prefix(name)
            .and_then(|rest| rest.strip_prefix('='))
        {
            return Ok(Some(value.to_owned()));
        }
    }
    Ok(None)
}

/// The client `native:coverage` asks as: `--platform` and `--form`, a phone
/// on Android when they are not given.
pub(crate) fn client_of(args: &[String]) -> Result<Client, BoxError> {
    use navree_native::{FormFactor, Platform};

    let platform = flag(args, "--platform")?.unwrap_or_else(|| "android".to_owned());
    let form = flag(args, "--form")?.unwrap_or_else(|| "phone".to_owned());
    let platform = Platform::parse(&platform).ok_or_else(|| {
        let known: Vec<&str> = Platform::ALL.iter().map(|p| p.as_str()).collect();
        format!("--platform `{platform}` is not one of {}", known.join(", "))
    })?;
    let form = FormFactor::parse(&form).ok_or_else(|| {
        let known: Vec<&str> = FormFactor::ALL.iter().map(|f| f.as_str()).collect();
        format!("--form `{form}` is not one of {}", known.join(", "))
    })?;
    Ok(Client::new(platform, form))
}

/// The plan in `--plan <json>`, or an empty one.
pub(crate) fn plan_of(args: &[String]) -> Result<Plan, BoxError> {
    match flag(args, "--plan")? {
        Some(json) => serde_json::from_str(&json)
            .map_err(|error| format!("--plan is not read: {error}").into()),
        None => Ok(Plan::default()),
    }
}

/// `native:coverage [--platform <p>] [--form <f>] [--plan <json>]`: asks every
/// page route of the app through its own router as a native client, and
/// prints the [`Report`] as JSON. `navree native:coverage` runs it and prints
/// the table.
pub(crate) async fn command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let client = client_of(&args)?;
    let plan = plan_of(&args)?;
    let options = ctx
        .get::<navree_http::HttpOptions>()
        .await
        .map_err(|error| format!("native:coverage needs HttpModule in the app: {error}"))?;
    let routes = navree_http::route_table(&ctx).await?;
    let router = navree_http::build_router(&ctx).await?;
    let report = survey(&router, &routes, &options.api_prefix, &plan, &client).await?;
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use http::StatusCode;
    use navree_http::RouteKind;
    use navree_native::{FormFactor, Platform};
    use tower::service_fn;

    use super::*;

    fn plan(values: &[(&str, &str)]) -> Plan {
        Plan {
            values: values
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            ..Plan::default()
        }
    }

    fn route(method: &str, path: &str) -> RouteMeta {
        RouteMeta {
            method: method.to_owned(),
            full_path: path.to_owned(),
            ..RouteMeta::default()
        }
    }

    fn phone() -> Client {
        Client::new(Platform::Android, FormFactor::Phone)
    }

    #[test]
    fn a_plan_is_read_and_a_typo_is_refused() {
        let plan: Plan = serde_json::from_str(
            r#"{"values":{"id":"1"},"routes":{"/orders/{id}/steps":{"id":"2"}},
                "also":["/orders?status=open"],"skip":["/admin/*"],
                "headers":{"cookie":"session=abc"}}"#,
        )
        .unwrap();
        assert_eq!(plan.values["id"], "1");
        assert_eq!(plan.routes["/orders/{id}/steps"]["id"], "2");
        assert_eq!(plan.also, ["/orders?status=open"]);
        assert!(serde_json::from_str::<Plan>(r#"{"value":{"id":"1"}}"#).is_err());
        assert_eq!(serde_json::from_str::<Plan>("{}").unwrap(), Plan::default());
    }

    #[test]
    fn a_pattern_is_filled_from_the_plan() {
        let plan = plan(&[("id", "1"), ("rest", "a/b.txt")]);
        assert_eq!(plan.fill("/"), Ok("/".to_owned()));
        assert_eq!(plan.fill("/orders/{id}"), Ok("/orders/1".to_owned()));
        assert_eq!(
            plan.fill("/orders/{id}/steps"),
            Ok("/orders/1/steps".to_owned())
        );
        // The rest of a path may hold slashes; a plain parameter may not.
        assert_eq!(plan.fill("/files/{*rest}"), Ok("/files/a/b.txt".to_owned()));
    }

    #[test]
    fn a_route_overrides_a_name() {
        let mut plan = plan(&[("id", "1")]);
        plan.routes.insert(
            "/orders/{id}/steps".to_owned(),
            BTreeMap::from([("id".to_owned(), "7".to_owned())]),
        );
        assert_eq!(plan.fill("/orders/{id}"), Ok("/orders/1".to_owned()));
        assert_eq!(
            plan.fill("/orders/{id}/steps"),
            Ok("/orders/7/steps".to_owned())
        );
    }

    #[test]
    fn a_missing_value_is_named() {
        let plan = plan(&[("id", "1")]);
        assert_eq!(
            plan.fill("/orders/{order}/lines/{line}"),
            Err(Fill::Missing(vec!["order".to_owned(), "line".to_owned()]))
        );
    }

    #[test]
    fn a_value_that_leaves_its_segment_is_refused() {
        for value in ["a/b", "a?b=1", "a#b", "a b", "", ".."] {
            let plan = plan(&[("id", value)]);
            assert!(
                matches!(plan.fill("/orders/{id}"), Err(Fill::Unsafe { .. })),
                "{value:?}"
            );
        }
        // Not out of a catch-all, either.
        let plan = plan(&[("rest", "a/../b")]);
        assert!(matches!(
            plan.fill("/files/{*rest}"),
            Err(Fill::Unsafe { .. })
        ));
    }

    #[test]
    fn glob_stars_match_any_text() {
        assert!(glob("/admin/*", "/admin/users"));
        assert!(glob("*/steps", "/orders/{id}/steps"));
        assert!(glob("/orders/*/steps", "/orders/{id}/steps"));
        assert!(glob("/orders", "/orders"));
        assert!(!glob("/orders", "/orders/1"));
        assert!(!glob("/admin/*", "/administer"));
        assert!(glob("*", "/anything"));
    }

    #[test]
    fn only_get_page_routes_are_asked() {
        let plan = plan(&[("id", "1")]);
        let pick = |meta: &RouteMeta| pick(meta, "/api", &plan);
        assert_eq!(
            pick(&route("GET", "/orders/{id}")),
            Pick::Ask("/orders/1".into())
        );
        assert_eq!(
            pick(&route("POST", "/orders/{id}/refund")),
            Pick::NotAPage("not a GET")
        );
        assert_eq!(
            pick(&route("GET", "/_navree/ui/core.js")),
            Pick::NotAPage("the toolkit's and the framework's own files")
        );
        assert_eq!(
            pick(&route("GET", "/.well-known/assetlinks.json")),
            Pick::NotAPage("the toolkit's and the framework's own files")
        );
        assert_eq!(
            pick(&route("GET", "/api/v1/deals")),
            Pick::NotAPage("routes under the API prefix")
        );
        // A route the app marked as a page is one wherever it is.
        let mut page = route("GET", "/api/pages/home");
        page.kind = RouteKind::Page;
        assert_eq!(pick(&page), Pick::Ask("/api/pages/home".into()));
        let mut data = route("GET", "/export");
        data.kind = RouteKind::Json;
        assert_eq!(pick(&data), Pick::NotAPage("data routes"));
        // `/apiary` is not under `/api`.
        assert_eq!(pick(&route("GET", "/apiary")), Pick::Ask("/apiary".into()));
    }

    #[test]
    fn a_skipped_pattern_says_so() {
        let mut plan = plan(&[]);
        plan.skip = vec!["/fragments/*".to_owned()];
        assert_eq!(
            pick(&route("GET", "/fragments/activity"), "/api", &plan),
            Pick::Skip("left out by `/fragments/*` in the plan".into())
        );
        assert_eq!(
            pick(&route("GET", "/orders/{id}"), "/api", &plan),
            Pick::Skip("needs a value for {id}".into())
        );
    }

    /// A router that answers from a table of `(path, status, content type,
    /// body, location)`, and for a browser the HTML of the page.
    fn router(
        answers: Vec<(&'static str, u16, &'static str, String)>,
    ) -> impl Service<Request<Body>, Response = Response, Error = Infallible, Future: Send> + Clone
    {
        service_fn(move |request: Request<Body>| {
            let answers = answers.clone();
            async move {
                let path = request
                    .uri()
                    .path_and_query()
                    .map_or("", |p| p.as_str())
                    .to_owned();
                let native = request.headers().contains_key(NATIVE_HEADER);
                let cookie = request
                    .headers()
                    .get(header::COOKIE)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_owned();
                let answer = answers.iter().find(|(at, ..)| *at == path);
                let mut response = match answer {
                    Some((_, status, kind, body)) if native => {
                        let mut response = Response::new(Body::from(body.clone()));
                        *response.status_mut() = StatusCode::from_u16(*status).unwrap();
                        response
                            .headers_mut()
                            .insert(header::CONTENT_TYPE, HeaderValue::from_static(kind));
                        if *status / 100 == 3 {
                            response
                                .headers_mut()
                                .insert(header::LOCATION, HeaderValue::from_static("/sign-in"));
                        }
                        response
                    }
                    Some(_) => {
                        let mut response = Response::new(Body::from("<html>the page</html>"));
                        response.headers_mut().insert(
                            header::CONTENT_TYPE,
                            HeaderValue::from_static("text/html; charset=utf-8"),
                        );
                        response
                    }
                    None => {
                        let mut response = Response::new(Body::empty());
                        *response.status_mut() = StatusCode::NOT_FOUND;
                        response
                    }
                };
                response.headers_mut().insert(
                    HeaderName::from_static("x-seen-cookie"),
                    HeaderValue::from_str(&cookie).unwrap(),
                );
                Ok(response)
            }
        })
    }

    fn screen(title: &str) -> String {
        format!(
            r#"{{"navree":1,"build":"x","vocab":0,"screen":{{"title":"{title}","body":{{"t":"stack","children":[{{"t":"text","value":"Hello"}}]}}}}}}"#
        )
    }

    fn web_view() -> String {
        r#"{"navree":1,"build":"x","vocab":0,"web":{"url":"/board","why":["tag:canvas"]}}"#
            .to_owned()
    }

    #[tokio::test]
    async fn a_screen_is_graded() {
        let router = router(vec![("/orders", 200, NATIVE_TYPE, screen("Orders"))]);
        let asked = ask(&router, "/orders", "/orders", &phone(), &[]).await;
        let Asked::Page(row) = asked else {
            panic!("{asked:?}");
        };
        assert_eq!(row.grade, "idiom");
        assert!(row.native);
        assert!(row.missing.is_empty());
        assert_eq!(row.envelope_bytes, screen("Orders").len());
        assert_eq!(row.html_bytes, "<html>the page</html>".len());

        // A screen with no title is a layout, and says what it lacks.
        let router = self::router(vec![("/x", 200, NATIVE_TYPE, screen(""))]);
        let Asked::Page(row) = ask(&router, "/x", "/x", &phone(), &[]).await else {
            panic!("a page");
        };
        assert_eq!(row.grade, "layout");
        assert_eq!(row.missing, ["the screen has no title"]);
    }

    #[tokio::test]
    async fn a_web_answer_keeps_its_why() {
        let router = router(vec![("/board", 200, NATIVE_TYPE, web_view())]);
        let Asked::Page(row) = ask(&router, "/board", "/board", &phone(), &[]).await else {
            panic!("a page");
        };
        assert_eq!(row.grade, "web");
        assert!(!row.native);
        assert_eq!(row.why, ["tag:canvas"]);
    }

    #[tokio::test]
    async fn a_redirect_to_sign_in_is_not_a_page() {
        let router = router(vec![("/orders", 303, "text/plain", String::new())]);
        let Asked::Skipped(reason) = ask(&router, "/orders", "/orders", &phone(), &[]).await else {
            panic!("skipped");
        };
        assert!(reason.contains("303 to /sign-in"), "{reason}");
        assert!(reason.contains("[coverage.headers]"), "{reason}");
        // Any other redirect is only not a page.
        assert!(
            refusal(303, Some("/deals")).contains("add the address it leads to"),
            "{}",
            refusal(303, Some("/deals"))
        );
        let router = self::router(vec![("/orders", 401, NATIVE_TYPE, "{}".to_owned())]);
        let Asked::Skipped(reason) = ask(&router, "/orders", "/orders", &phone(), &[]).await else {
            panic!("skipped");
        };
        assert!(reason.contains("401"), "{reason}");
    }

    #[tokio::test]
    async fn an_address_that_does_not_exist_is_skipped_with_the_line_to_fix() {
        let router = router(Vec::new());
        let Asked::Skipped(reason) = ask(&router, "/orders/{id}", "/orders/9", &phone(), &[]).await
        else {
            panic!("skipped");
        };
        assert!(reason.starts_with("answered 404"), "{reason}");
    }

    #[tokio::test]
    async fn operations_with_no_screen_are_not_a_page() {
        let redirect =
            r#"{"navree":1,"build":"x","vocab":0,"ops":[{"op":"navigate","href":"/deals"}]}"#;
        let router = router(vec![("/old", 200, NATIVE_TYPE, redirect.to_owned())]);
        let Asked::Skipped(reason) = ask(&router, "/old", "/old", &phone(), &[]).await else {
            panic!("skipped");
        };
        assert!(
            reason.starts_with("answered a redirect to /deals"),
            "{reason}"
        );
        let none = r#"{"navree":1,"build":"x","vocab":0}"#;
        let router = self::router(vec![("/x", 200, NATIVE_TYPE, none.to_owned())]);
        assert_eq!(
            ask(&router, "/x", "/x", &phone(), &[]).await,
            Asked::Skipped("answered operations, not a screen".to_owned())
        );
    }

    #[tokio::test]
    async fn a_json_route_is_not_a_page() {
        let router = router(vec![("/prices", 200, "application/json", "[]".to_owned())]);
        assert_eq!(
            ask(&router, "/prices", "/prices", &phone(), &[]).await,
            Asked::NotAPage("routes that answer application/json".to_owned())
        );
    }

    #[tokio::test]
    async fn an_event_stream_is_not_read() {
        let router = router(vec![(
            "/broadcasting/stream",
            200,
            "text/event-stream",
            "data: x\n\n".to_owned(),
        )]);
        assert_eq!(
            ask(&router, "/s", "/broadcasting/stream", &phone(), &[]).await,
            Asked::NotAPage("event streams".to_owned())
        );
    }

    #[tokio::test]
    async fn an_answer_that_is_not_an_envelope_is_skipped() {
        let router = router(vec![("/odd", 200, NATIVE_TYPE, "not json".to_owned())]);
        let Asked::Skipped(reason) = ask(&router, "/odd", "/odd", &phone(), &[]).await else {
            panic!("skipped");
        };
        assert!(
            reason.starts_with("the answer is not an envelope"),
            "{reason}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_slow_route_is_skipped() {
        let router = service_fn(|_request: Request<Body>| async {
            std::future::pending::<()>().await;
            Ok::<Response, Infallible>(Response::new(Body::empty()))
        });
        let Asked::Skipped(reason) = ask(&router, "/slow", "/slow", &phone(), &[]).await else {
            panic!("skipped");
        };
        assert_eq!(reason, "no answer in 20 seconds");
    }

    #[tokio::test]
    async fn a_header_of_the_plan_reaches_the_route() {
        let mut plan = plan(&[]);
        plan.headers
            .insert("cookie".to_owned(), "session=abc".to_owned());
        let extra = headers_of(&plan).unwrap();
        let router = router(vec![("/orders", 200, NATIVE_TYPE, screen("Orders"))]);
        // Seen by the route: the router echoes the cookie it was sent.
        let request = request("/orders", Some(&phone()), &extra).unwrap();
        let response = send(&router, request).await.unwrap();
        assert_eq!(response.headers()["x-seen-cookie"], "session=abc");
        plan.headers.insert("bad header".to_owned(), "x".to_owned());
        assert!(headers_of(&plan).unwrap_err().contains("not a header name"));
    }

    #[tokio::test]
    async fn the_survey_lists_what_it_asked_skipped_and_counted() {
        let router = router(vec![
            ("/", 200, NATIVE_TYPE, screen("Home")),
            ("/orders/1", 200, NATIVE_TYPE, screen("Order")),
            ("/board", 200, NATIVE_TYPE, web_view()),
            ("/orders?status=open", 200, NATIVE_TYPE, screen("Open")),
        ]);
        let routes = vec![
            route("GET", "/"),
            route("GET", "/orders/{id}"),
            route("GET", "/orders/{id}/lines/{line}"),
            route("GET", "/board"),
            route("GET", "/api/v1/orders"),
            route("POST", "/orders"),
            route("POST", "/orders/{id}/cancel"),
            route("GET", "/_navree/ui/core.js"),
        ];
        let mut plan = plan(&[("id", "1")]);
        plan.also = vec!["/orders?status=open".to_owned()];
        let report = survey(&router, &routes, "/api", &plan, &phone())
            .await
            .unwrap();
        let asked: Vec<(&str, &str, &str)> = report
            .pages
            .iter()
            .map(|row| (row.route.as_str(), row.path.as_str(), row.grade.as_str()))
            .collect();
        assert_eq!(
            asked,
            [
                ("/", "/", "idiom"),
                ("/orders/{id}", "/orders/1", "idiom"),
                ("/board", "/board", "web"),
                ("/orders?status=open", "/orders?status=open", "idiom"),
            ]
        );
        assert_eq!(report.skipped.len(), 1);
        assert_eq!(report.skipped[0].route, "/orders/{id}/lines/{line}");
        assert_eq!(report.skipped[0].reason, "needs a value for {line}");
        let counted: Vec<(&str, usize)> = report
            .not_pages
            .iter()
            .map(|n| (n.reason.as_str(), n.count))
            .collect();
        assert_eq!(
            counted,
            [
                ("routes under the API prefix", 1),
                ("not a GET", 2),
                ("the toolkit's and the framework's own files", 1),
            ]
        );
        assert_eq!(
            (report.platform.as_str(), report.form.as_str()),
            ("android", "phone")
        );
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["format"], "navree-native-coverage");
        assert_eq!(json["version"], 1);
    }

    #[tokio::test]
    async fn an_extra_address_must_be_an_address_of_the_app() {
        let router = router(Vec::new());
        let mut plan = plan(&[]);
        plan.also = vec!["https://example.com/x".to_owned()];
        let error = survey(&router, &[], "/api", &plan, &phone())
            .await
            .unwrap_err();
        assert!(error.contains("not `https://example.com/x`"), "{error}");
    }

    #[test]
    fn the_client_is_a_phone_on_android_unless_told() {
        let args = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let client = client_of(&args(&[])).unwrap();
        assert_eq!(
            (client.platform, client.form),
            (Platform::Android, FormFactor::Phone)
        );
        let client = client_of(&args(&["--platform", "ios", "--form=tablet"])).unwrap();
        assert_eq!(
            (client.platform, client.form),
            (Platform::Ios, FormFactor::Tablet)
        );
        let error = client_of(&args(&["--platform", "symbian"]))
            .unwrap_err()
            .to_string();
        assert!(error.contains("`symbian` is not one of ios"), "{error}");
        assert!(client_of(&args(&["--form"])).is_err());
        assert_eq!(plan_of(&args(&[])).unwrap(), Plan::default());
        assert!(plan_of(&args(&["--plan", "{"])).is_err());
    }
}
