//! The dashboard: server-rendered pages at `lighthouse.path`, its sign-in
//! screen, and its stylesheet and script.

use std::sync::Arc;

use http::HeaderMap;
use http::header::{HeaderName, HeaderValue};
use http::request::Parts;
use http::{Method, StatusCode};
use navree_access::{Abilities, Subject as _};
use navree_auth::{AuthError, AuthService, ClientMeta, LoginOutcome, TwoFactorProof};
use navree_crypto::{Encrypter, constant_time_eq, random_token};
use navree_http::extract::FromRequestParts;
use navree_http::response::{Html, IntoResponse, Redirect, Response};
use navree_http::{AppError, ClientIp, Controller, CurrentPrincipal, Form, Path, Query, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree_view::Views;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::api::{IssueFilterQuery, PageQuery, PeriodQuery, RouteQuery};
use crate::chart::{axis_labels, bars, release_lines, sparkline, waterfall};
use crate::module::{CONFIGURE, LighthouseSubject, RESOLVE, VIEW};
use crate::options::{Period, StoreKind};
use crate::service::Lighthouse;
use crate::session::{DashboardSession, LOGIN_CSRF_COOKIE, Sessions, cookie_value};
use crate::store::{IssueFilter, LighthouseStore};
use crate::{IssueKind, IssueStatus};

/// The stylesheet, served at `{path}/assets/lighthouse.css`.
pub const DASHBOARD_CSS: &str = include_str!("../assets/lighthouse.css");
/// The script, served at `{path}/assets/lighthouse.js`.
pub const DASHBOARD_JS: &str = include_str!("../assets/lighthouse.js");

/// The pages' Content Security Policy: only the dashboard's own stylesheet,
/// script and API.
pub const CONTENT_SECURITY_POLICY: &str = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'";

const TEMPLATES: [(&str, &str); 16] = [
    (
        "lighthouse/layout.html",
        include_str!("../templates/layout.html"),
    ),
    (
        "lighthouse/overview.html",
        include_str!("../templates/overview.html"),
    ),
    (
        "lighthouse/issues.html",
        include_str!("../templates/issues.html"),
    ),
    (
        "lighthouse/issue.html",
        include_str!("../templates/issue.html"),
    ),
    (
        "lighthouse/requests.html",
        include_str!("../templates/requests.html"),
    ),
    (
        "lighthouse/route.html",
        include_str!("../templates/route.html"),
    ),
    (
        "lighthouse/trace.html",
        include_str!("../templates/trace.html"),
    ),
    (
        "lighthouse/queries.html",
        include_str!("../templates/queries.html"),
    ),
    (
        "lighthouse/jobs.html",
        include_str!("../templates/jobs.html"),
    ),
    (
        "lighthouse/outgoing.html",
        include_str!("../templates/outgoing.html"),
    ),
    (
        "lighthouse/login.html",
        include_str!("../templates/login.html"),
    ),
    (
        "lighthouse/two_factor.html",
        include_str!("../templates/two_factor.html"),
    ),
    (
        "lighthouse/forbidden.html",
        include_str!("../templates/forbidden.html"),
    ),
    (
        "lighthouse/alerts.html",
        include_str!("../templates/alerts.html"),
    ),
    (
        "lighthouse/releases.html",
        include_str!("../templates/releases.html"),
    ),
    (
        "lighthouse/settings.html",
        include_str!("../templates/settings.html"),
    ),
];

/// Adds the pages' security headers.
pub(crate) fn secure_headers(headers: &mut HeaderMap) {
    let pairs = [
        ("content-security-policy", CONTENT_SECURITY_POLICY),
        ("cache-control", "no-store"),
        ("x-frame-options", "DENY"),
        ("referrer-policy", "same-origin"),
        ("x-content-type-options", "nosniff"),
    ];
    for (name, value) in pairs {
        headers.insert(
            HeaderName::from_static(name),
            HeaderValue::from_static(value),
        );
    }
}

/// An HTML response with the pages' security headers.
fn html(status: StatusCode, body: String) -> Response {
    let mut response = (status, Html(body)).into_response();
    secure_headers(response.headers_mut());
    response
}

/// The page a signed-in user without `view` gets (403).
pub(crate) fn forbidden_page(mount: &str) -> String {
    views()
        .and_then(|views| {
            views
                .render("lighthouse/forbidden.html", json!({ "mount": mount }))
                .ok()
        })
        .unwrap_or_else(|| "<!doctype html><title>Forbidden</title><p>Forbidden</p>".to_owned())
}

/// The compiled templates.
fn views() -> Option<&'static Views> {
    static VIEWS: std::sync::OnceLock<Option<Views>> = std::sync::OnceLock::new();
    VIEWS
        .get_or_init(|| match Views::from_templates(TEMPLATES) {
            Ok(views) => Some(views),
            Err(error) => {
                tracing::error!(target: "navree::lighthouse", %error, "the dashboard templates do not compile");
                None
            }
        })
        .as_ref()
}

/// Who is looking at a page: the signed-in user, their CSRF token (when
/// signed in through the dashboard) and whether they may change issues.
pub(crate) struct Viewer {
    user_id: Uuid,
    csrf: String,
    can_resolve: bool,
    pub(crate) can_configure: bool,
    path: String,
}

impl FromRequestParts for Viewer {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        let CurrentPrincipal(principal) = CurrentPrincipal::from_parts(parts)?;
        let abilities = Abilities::from_request_parts(parts).await?;
        Ok(Viewer {
            user_id: principal.user_id,
            csrf: parts
                .extensions
                .get::<DashboardSession>()
                .map(|session| session.csrf.clone())
                .unwrap_or_default(),
            can_resolve: abilities.can(RESOLVE, LighthouseSubject::SUBJECT, None),
            can_configure: abilities.can(CONFIGURE, LighthouseSubject::SUBJECT, None),
            path: parts.uri.path().to_owned(),
        })
    }
}

/// The dashboard's pages, sign-in and assets, mounted at `lighthouse.path`
/// when `lighthouse.dashboard` is on. Pages check `view` on the subject
/// `Lighthouse`; a guest is sent to the sign-in screen, a user without
/// `view` gets a 403 page.
pub struct LighthouseDashboardController {
    pub(crate) lighthouse: Arc<Lighthouse>,
    auth: Option<Arc<AuthService>>,
    sessions: Option<Sessions>,
    /// `observe.tracing.trace_url`: the trace backend's page, with
    /// `{trace_id}`.
    trace_url: Option<String>,
}

impl Injectable for LighthouseDashboardController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Lighthouse>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            let lighthouse = r.get::<Lighthouse>().await?;
            let app = r.app();
            let mount = lighthouse.options().mount_path().to_owned();
            let secure = lighthouse
                .store()
                .is_some_and(|store| store.base_url().starts_with("https://"));
            let sessions = app
                .try_get::<Encrypter>()
                .await
                .map(|encrypter| Sessions::new(encrypter, &mount, secure));
            Ok(LighthouseDashboardController {
                auth: app.try_get::<AuthService>().await,
                trace_url: app
                    .try_get::<navree_config::Config>()
                    .await
                    .and_then(|config| config.get_string("observe.tracing.trace_url"))
                    .filter(|url| url.contains("{trace_id}")),
                lighthouse,
                sessions,
            })
        })
    }
}

/// The sign-in form. `Debug` never shows the password or the CSRF token.
#[derive(Clone, Default, Deserialize)]
#[serde(default)]
pub(crate) struct LoginForm {
    email: String,
    password: String,
    csrf: String,
    next: String,
}

/// The two-factor form. `Debug` never shows the challenge, the code or the
/// CSRF token.
#[derive(Clone, Default, Deserialize)]
#[serde(default)]
pub(crate) struct TwoFactorForm {
    challenge: String,
    code: String,
    csrf: String,
    next: String,
}

impl std::fmt::Debug for LoginForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoginForm")
            .field("email", &self.email)
            .field("password", &"[redacted]")
            .field("csrf", &"[redacted]")
            .field("next", &self.next)
            .finish()
    }
}

impl std::fmt::Debug for TwoFactorForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TwoFactorForm")
            .field("challenge", &"[redacted]")
            .field("code", &"[redacted]")
            .field("csrf", &"[redacted]")
            .field("next", &self.next)
            .finish()
    }
}

/// `?next=` on the sign-in screen.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct NextQuery {
    next: String,
}

macro_rules! page {
    ($this:expr, $path:expr, $name:expr, |$c:ident, $viewer:ident $(, $arg:ident : $ty:ty)*| $body:expr) => {{
        let this = $this.clone();
        RouteDef::on(Method::GET, $path, move |$viewer: Viewer $(, $arg: $ty)*| {
            let this = this.clone();
            async move {
                let $c: &LighthouseDashboardController = &this;
                let result: Result<Response, AppError> = $body;
                result
            }
        })
        .can(VIEW, LighthouseSubject::SUBJECT, None)
        .name($name)
    }};
}

impl Controller for LighthouseDashboardController {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let options = self.lighthouse.options();
        if !options.dashboard
            || options.store == StoreKind::Off
            || self.lighthouse.store().is_none()
        {
            return Vec::new();
        }
        let mount = options.mount_path().to_owned();
        let path = |rest: &str| format!("!{mount}{rest}");
        let (login_form, login, two_factor, logout) =
            (self.clone(), self.clone(), self.clone(), self.clone());
        vec![
            page!(self, path(""), "lighthouse.overview", |c,
                                                          viewer,
                                                          q: Query<
                PeriodQuery,
            >| {
                c.overview(viewer, q.period()?).await
            }),
            page!(self, path("/alerts"), "lighthouse.alerts", |c,
                                                               viewer,
                                                               q: Query<
                PageQuery,
            >| {
                c.alerts(viewer, q.0).await
            }),
            page!(
                self,
                path("/releases"),
                "lighthouse.releases",
                |c, viewer, q: Query<crate::config_pages::ReleaseQuery>| {
                    c.releases(viewer, q.0).await
                }
            ),
            page!(
                self,
                path("/settings"),
                "lighthouse.settings",
                |c, viewer| { c.settings(viewer).await }
            ),
            page!(self, path("/issues"), "lighthouse.issues", |c,
                                                               viewer,
                                                               q: Query<
                IssueFilterQuery,
            >| {
                c.issues(viewer, q.0).await
            }),
            page!(
                self,
                path("/issues/{id}"),
                "lighthouse.issues.show",
                |c, viewer, id: Path<Uuid>| { c.issue(viewer, id.0).await }
            ),
            page!(
                self,
                path("/requests"),
                "lighthouse.requests",
                |c, viewer, q: Query<PeriodQuery>| { c.requests(viewer, q.period()?).await }
            ),
            page!(
                self,
                path("/requests/route"),
                "lighthouse.requests.show",
                |c, viewer, q: Query<RouteQuery>| {
                    let period = PeriodQuery {
                        period: q.period.clone(),
                    }
                    .period()?;
                    c.route(viewer, &q.endpoint, period).await
                }
            ),
            page!(
                self,
                path("/traces/{id}"),
                "lighthouse.traces.show",
                |c, viewer, id: Path<Uuid>| { c.trace(viewer, id.0).await }
            ),
            page!(
                self,
                path("/queries"),
                "lighthouse.queries",
                |c, viewer, q: Query<PeriodQuery>| { c.queries(viewer, q.period()?).await }
            ),
            page!(self, path("/jobs"), "lighthouse.jobs", |c,
                                                           viewer,
                                                           q: Query<
                PeriodQuery,
            >| {
                c.jobs(viewer, q.period()?).await
            }),
            page!(
                self,
                path("/outgoing"),
                "lighthouse.outgoing",
                |c, viewer, q: Query<PeriodQuery>| { c.outgoing(viewer, q.period()?).await }
            ),
            RouteDef::on(
                Method::GET,
                path("/login"),
                move |headers: HeaderMap, q: Query<NextQuery>| {
                    let this = login_form.clone();
                    async move { this.login_form(&headers, &q.next) }
                },
            )
            .public()
            .name("lighthouse.login"),
            RouteDef::on(
                Method::POST,
                path("/login"),
                move |headers: HeaderMap, ip: ClientIp, form: Form<LoginForm>| {
                    let this = login.clone();
                    async move { this.login(&headers, ip, form.0).await }
                },
            )
            .public()
            .name("lighthouse.login.attempt"),
            RouteDef::on(
                Method::POST,
                path("/login/two-factor"),
                move |headers: HeaderMap, ip: ClientIp, form: Form<TwoFactorForm>| {
                    let this = two_factor.clone();
                    async move { this.two_factor(&headers, ip, form.0).await }
                },
            )
            .public()
            .name("lighthouse.login.two_factor"),
            RouteDef::on(Method::POST, path("/logout"), move || {
                let this = logout.clone();
                async move { this.logout() }
            })
            .authenticated()
            .name("lighthouse.logout"),
            RouteDef::on(Method::GET, path("/assets/lighthouse.css"), || async {
                asset("text/css; charset=utf-8", DASHBOARD_CSS)
            })
            .public()
            .name("lighthouse.assets.css"),
            RouteDef::on(Method::GET, path("/assets/lighthouse.js"), || async {
                asset("text/javascript; charset=utf-8", DASHBOARD_JS)
            })
            .public()
            .name("lighthouse.assets.js"),
        ]
    }
}

/// A static asset.
fn asset(content_type: &'static str, body: &'static str) -> Response {
    let mut response = body.into_response();
    let headers = response.headers_mut();
    headers.insert(
        http::header::CONTENT_TYPE,
        HeaderValue::from_static(content_type),
    );
    headers.insert(
        http::header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=3600"),
    );
    response
}

impl LighthouseDashboardController {
    pub(crate) fn store(&self) -> Result<&LighthouseStore, AppError> {
        self.lighthouse
            .store()
            .ok_or_else(|| AppError::service_unavailable("lighthouse.store is off"))
    }

    /// The trace backend's page of `trace_id`, when `observe.tracing.trace_url`
    /// is set.
    fn trace_link(&self, trace_id: Option<&str>) -> Option<String> {
        Some(self.trace_url.as_ref()?.replace("{trace_id}", trace_id?))
    }

    pub(crate) fn mount(&self) -> &str {
        self.lighthouse.options().mount_path()
    }

    /// Renders `template` inside the layout's context.
    pub(crate) async fn render(
        &self,
        viewer: &Viewer,
        template: &str,
        page: &str,
        title: &str,
        period: Option<Period>,
        data: Value,
    ) -> Result<Response, AppError> {
        let store = self.store()?;
        let user = match &self.auth {
            Some(auth) => auth
                .find_user(viewer.user_id)
                .await
                .ok()
                .flatten()
                .map(|user| user.email),
            None => None,
        };
        let open_issues = store.open_issue_count().await?;
        // The release in the header: the newest deploy, else the label this
        // process was given; never a made-up one.
        let current = store.current_release_label().await?;
        let release = match &current {
            Some((name, _)) => Some(name.clone()),
            None => Some(store.release().to_owned()).filter(|r| r != "unknown" && !r.is_empty()),
        };
        let failed_alerts = store.alert_counts(24).await?.failed;
        let mut context = json!({
            "mount": self.mount(),
            "title": title,
            "page": page,
            "path": viewer.path,
            "period_base": format!("{}?", viewer.path),
            "csrf": viewer.csrf,
            "user": user.unwrap_or_else(|| viewer.user_id.to_string()),
            "environment": self.lighthouse.options().environment_label(),
            "release": release,
            "release_deployed": current.as_ref().map(|(_, at)| ago(*at)),
            "failed_alerts": failed_alerts,
            "can_configure": viewer.can_configure,
            "open_issues": open_issues,
            "dropped": self.lighthouse.dropped(),
            "period": period.map(Period::as_str),
            "periods": Period::ALL.iter().map(|p| p.as_str()).collect::<Vec<_>>(),
            "can_resolve": viewer.can_resolve,
        });
        if let (Value::Object(context), Value::Object(data)) = (&mut context, data) {
            context.extend(data);
        }
        let views =
            views().ok_or_else(|| AppError::internal("the dashboard templates do not compile"))?;
        let body = views
            .render(template, context)
            .map_err(AppError::internal)?;
        Ok(html(StatusCode::OK, body))
    }

    async fn overview(&self, viewer: Viewer, period: Period) -> Result<Response, AppError> {
        let mut overview = self.store()?.overview(period).await?;
        overview.dropped = self.lighthouse.dropped();
        let requests: Vec<i64> = overview.timeline.iter().map(|p| p.count).collect();
        let errors: Vec<i64> = overview.timeline.iter().map(|p| p.errors).collect();
        let series = &overview.series;
        let line = |values: &[f64]| sparkline(values, 80.0, 24.0);
        let failed: Vec<f64> = series.failed_jobs.iter().map(|n| *n as f64).collect();
        let lines = overview.timeline.first().map_or_else(Vec::new, |first| {
            release_lines(
                &overview.releases,
                first.at,
                i64::from(period.resolution()),
                overview.timeline.len(),
                960.0,
            )
        });
        let feed: Vec<Value> = overview
            .feed
            .iter()
            .map(|item| {
                let mut row = serde_json::to_value(item).unwrap_or(Value::Null);
                row["age"] = json!(age_short(item.at));
                row
            })
            .collect();
        let top = self
            .store()?
            .issues(&IssueFilter {
                status: Some("open".to_owned()),
                per_page: Some(5),
                ..IssueFilter::default()
            })
            .await?;
        let top_issues: Vec<Value> = top
            .data
            .iter()
            .map(|summary| {
                let mut row = serde_json::to_value(summary).unwrap_or(Value::Null);
                row["spark"] = json!(bars(&summary.sparkline, 88.0, 28.0));
                row["seen"] = json!(age_short(summary.issue.last_seen));
                row
            })
            .collect();
        let data = json!({
            "overview": overview,
            "error_percent": round1(overview.error_rate * 100.0),
            "requests_bars": bars(&requests, 960.0, 120.0),
            "error_bars": bars(&errors, 960.0, 40.0),
            "axis": axis_labels(&overview.timeline, period),
            "release_lines": lines,
            "spark_requests": line(&series.requests_per_minute),
            "spark_errors": line(&series.error_rate),
            "spark_p95": line(&series.p95_ms),
            "spark_failed": line(&failed),
            "feed": feed,
            "top_issues": top_issues,
        });
        self.render(
            &viewer,
            "lighthouse/overview.html",
            "overview",
            "Overview",
            Some(period),
            data,
        )
        .await
    }

    async fn issues(&self, viewer: Viewer, query: IssueFilterQuery) -> Result<Response, AppError> {
        let filter = query.filter();
        let page = self.store()?.issues(&filter).await?;
        let rows: Vec<Value> = page
            .data
            .iter()
            .map(|summary| {
                let mut row = serde_json::to_value(summary).unwrap_or(Value::Null);
                row["spark"] = json!(bars(&summary.sparkline, 96.0, 20.0));
                row
            })
            .collect();
        let data = json!({
            "issues": rows,
            "meta": page.meta,
            "filter": filter,
            "query": query.encoded_without_page(),
            "statuses": IssueStatus::ALL.iter().map(|s| json!({"value": s.as_str(), "label": s.label()})).collect::<Vec<_>>(),
            "kinds": IssueKind::ALL.iter().map(|k| json!({"value": k.as_str(), "label": k.label()})).collect::<Vec<_>>(),
        });
        self.render(
            &viewer,
            "lighthouse/issues.html",
            "issues",
            "Issues",
            None,
            data,
        )
        .await
    }

    async fn issue(&self, viewer: Viewer, id: Uuid) -> Result<Response, AppError> {
        let store = self.store()?;
        let detail = store.issue_detail(id).await?;
        let occurrences = store.occurrences(id, 1, 25).await?;
        let counts: Vec<i64> = detail.timeline.iter().map(|p| p.count).collect();
        let latest_context = detail
            .latest
            .as_ref()
            .map(|latest| serde_json::to_string_pretty(&latest.context).unwrap_or_default());
        let title = detail.issue.title.clone();
        let trace_id = detail
            .latest
            .as_ref()
            .and_then(|latest| latest.trace_id.clone());
        let data = json!({
            "issue": detail,
            "timeline_bars": bars(&counts, 960.0, 80.0),
            "occurrences": occurrences.data,
            "occurrences_total": occurrences.meta.total,
            "latest_context": latest_context,
            "trace_link": self.trace_link(trace_id.as_deref()),
        });
        self.render(
            &viewer,
            "lighthouse/issue.html",
            "issues",
            &title,
            None,
            data,
        )
        .await
    }

    async fn requests(&self, viewer: Viewer, period: Period) -> Result<Response, AppError> {
        let routes = self.store()?.routes(period).await?;
        let rows: Vec<Value> = routes
            .iter()
            .map(|route| {
                let mut row = serde_json::to_value(route).unwrap_or(Value::Null);
                row["error_percent"] = json!(round1(route.error_rate * 100.0));
                row["link"] = json!(encode(&route.endpoint));
                row
            })
            .collect();
        self.render(
            &viewer,
            "lighthouse/requests.html",
            "requests",
            "Requests",
            Some(period),
            json!({ "routes": rows }),
        )
        .await
    }

    async fn route(
        &self,
        viewer: Viewer,
        endpoint: &str,
        period: Period,
    ) -> Result<Response, AppError> {
        let detail = self.store()?.route(endpoint, period).await?;
        let counts: Vec<i64> = detail.timeline.iter().map(|p| p.count).collect();
        let errors: Vec<i64> = detail.timeline.iter().map(|p| p.errors).collect();
        let data = json!({
            "route": detail,
            "error_percent": round1(detail.stats.error_rate * 100.0),
            "endpoint": endpoint,
            "link": encode(endpoint),
            "period_base": format!("{}?endpoint={}&", viewer.path, encode(endpoint)),
            "requests_bars": bars(&counts, 960.0, 100.0),
            "error_bars": bars(&errors, 960.0, 40.0),
        });
        self.render(
            &viewer,
            "lighthouse/route.html",
            "requests",
            endpoint,
            Some(period),
            data,
        )
        .await
    }

    async fn trace(&self, viewer: Viewer, id: Uuid) -> Result<Response, AppError> {
        let sample = self.store()?.sample(id).await?;
        let rows = waterfall(&sample.spans, sample.summary.duration_ms, 1000.0);
        let title = sample.summary.name.clone();
        let data = json!({
            "trace": sample,
            "rows": rows,
            "context": serde_json::to_string_pretty(&sample.context).unwrap_or_default(),
            "trace_link": self.trace_link(sample.summary.trace_id.as_deref()),
        });
        self.render(
            &viewer,
            "lighthouse/trace.html",
            "requests",
            &title,
            None,
            data,
        )
        .await
    }

    async fn queries(&self, viewer: Viewer, period: Period) -> Result<Response, AppError> {
        let queries = self.store()?.queries(period).await?;
        self.render(
            &viewer,
            "lighthouse/queries.html",
            "queries",
            "Queries",
            Some(period),
            json!({ "queries": queries }),
        )
        .await
    }

    async fn jobs(&self, viewer: Viewer, period: Period) -> Result<Response, AppError> {
        let jobs = self.store()?.jobs(period).await?;
        self.render(
            &viewer,
            "lighthouse/jobs.html",
            "jobs",
            "Jobs",
            Some(period),
            json!({ "jobs": jobs }),
        )
        .await
    }

    async fn outgoing(&self, viewer: Viewer, period: Period) -> Result<Response, AppError> {
        let hosts = self.store()?.outgoing(period).await?;
        let rows: Vec<Value> = hosts
            .iter()
            .map(|host| {
                let mut row = serde_json::to_value(host).unwrap_or(Value::Null);
                row["error_percent"] = json!(round1(host.error_rate * 100.0));
                row
            })
            .collect();
        self.render(
            &viewer,
            "lighthouse/outgoing.html",
            "outgoing",
            "Outgoing",
            Some(period),
            json!({ "hosts": rows }),
        )
        .await
    }

    // -----------------------------------------------------------------
    // Signing in
    // -----------------------------------------------------------------

    /// Where to go after signing in: `next` when it is a dashboard page.
    fn next_page(&self, next: &str) -> String {
        let mount = self.mount();
        let safe = crate::http::under(next.split('?').next().unwrap_or(""), mount)
            && !next.contains("//")
            && !next.contains('\\');
        if safe {
            next.to_owned()
        } else {
            mount.to_owned()
        }
    }

    /// Renders the sign-in screen with a fresh form token.
    fn login_page(&self, status: StatusCode, next: &str, error: Option<&str>) -> Response {
        let token = random_token(32);
        let body = views()
            .and_then(|views| {
                views
                    .render(
                        "lighthouse/login.html",
                        json!({
                            "mount": self.mount(),
                            "csrf": token,
                            "next": self.next_page(next),
                            "error": error,
                            "available": self.auth.is_some() && self.sessions.is_some(),
                        }),
                    )
                    .ok()
            })
            .unwrap_or_default();
        let mut response = html(status, body);
        if let Some(sessions) = &self.sessions
            && let Ok(value) = HeaderValue::from_str(&sessions.login_csrf(&token))
        {
            response
                .headers_mut()
                .append(http::header::SET_COOKIE, value);
        }
        response
    }

    fn login_form(&self, headers: &HeaderMap, next: &str) -> Result<Response, AppError> {
        if self
            .sessions
            .as_ref()
            .and_then(|sessions| sessions.read(headers))
            .is_some()
        {
            return Ok(Redirect::to(&self.next_page(next)).into_response());
        }
        Ok(self.login_page(StatusCode::OK, next, None))
    }

    /// Whether the form's token matches the sign-in cookie.
    fn form_token_ok(headers: &HeaderMap, token: &str) -> bool {
        cookie_value(headers, LOGIN_CSRF_COOKIE).is_some_and(|cookie| {
            !token.is_empty() && constant_time_eq(cookie.as_bytes(), token.as_bytes())
        })
    }

    fn client_meta(headers: &HeaderMap, ip: ClientIp) -> ClientMeta {
        ClientMeta {
            ip: Some(ip.0.to_string()),
            user_agent: headers
                .get(http::header::USER_AGENT)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned),
            request_id: None,
        }
    }

    async fn login(
        &self,
        headers: &HeaderMap,
        ip: ClientIp,
        form: LoginForm,
    ) -> Result<Response, AppError> {
        let (Some(auth), Some(_)) = (&self.auth, &self.sessions) else {
            return Ok(self.login_page(
                StatusCode::SERVICE_UNAVAILABLE,
                &form.next,
                Some(UNAVAILABLE),
            ));
        };
        if !Self::form_token_ok(headers, &form.csrf) {
            return Ok(self.login_page(StatusCode::FORBIDDEN, &form.next, Some(EXPIRED)));
        }
        let meta = Self::client_meta(headers, ip);
        match auth.attempt_login(&form.email, &form.password, &meta).await {
            Ok(LoginOutcome::Authenticated { user, tokens }) => {
                self.signed_in(auth, user.id, &tokens.refresh_token, &form.next)
                    .await
            }
            Ok(LoginOutcome::TwoFactorRequired {
                challenge_token, ..
            }) => Ok(self.two_factor_page(StatusCode::OK, &challenge_token, &form.next, None)),
            Err(AuthError::InvalidCredentials) => Ok(self.login_page(
                StatusCode::UNAUTHORIZED,
                &form.next,
                Some("These credentials do not match an account."),
            )),
            Err(AuthError::TooManyAttempts { retry_after }) => Ok(self.login_page(
                StatusCode::TOO_MANY_REQUESTS,
                &form.next,
                Some(&format!(
                    "Too many sign-in attempts. Try again in {} seconds.",
                    retry_after.as_secs().max(1)
                )),
            )),
            Err(error) => Err(AppError::internal(error)),
        }
    }

    fn two_factor_page(
        &self,
        status: StatusCode,
        challenge: &str,
        next: &str,
        error: Option<&str>,
    ) -> Response {
        let token = random_token(32);
        let body = views()
            .and_then(|views| {
                views
                    .render(
                        "lighthouse/two_factor.html",
                        json!({
                            "mount": self.mount(),
                            "csrf": token,
                            "challenge": challenge,
                            "next": self.next_page(next),
                            "error": error,
                        }),
                    )
                    .ok()
            })
            .unwrap_or_default();
        let mut response = html(status, body);
        if let Some(sessions) = &self.sessions
            && let Ok(value) = HeaderValue::from_str(&sessions.login_csrf(&token))
        {
            response
                .headers_mut()
                .append(http::header::SET_COOKIE, value);
        }
        response
    }

    async fn two_factor(
        &self,
        headers: &HeaderMap,
        ip: ClientIp,
        form: TwoFactorForm,
    ) -> Result<Response, AppError> {
        let (Some(auth), Some(_)) = (&self.auth, &self.sessions) else {
            return Ok(self.login_page(
                StatusCode::SERVICE_UNAVAILABLE,
                &form.next,
                Some(UNAVAILABLE),
            ));
        };
        if !Self::form_token_ok(headers, &form.csrf) {
            return Ok(self.login_page(StatusCode::FORBIDDEN, &form.next, Some(EXPIRED)));
        }
        let code = form.code.trim().to_owned();
        let proof = if code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit()) {
            TwoFactorProof::Code(code)
        } else {
            TwoFactorProof::RecoveryCode(code)
        };
        let meta = Self::client_meta(headers, ip);
        match auth.verify_two_factor(&form.challenge, proof, &meta).await {
            Ok((user, tokens)) => {
                self.signed_in(auth, user.id, &tokens.refresh_token, &form.next)
                    .await
            }
            Err(AuthError::InvalidTwoFactorCode) => Ok(self.two_factor_page(
                StatusCode::UNAUTHORIZED,
                &form.challenge,
                &form.next,
                Some("That code is not valid."),
            )),
            Err(AuthError::TooManyAttempts { retry_after }) => Ok(self.login_page(
                StatusCode::TOO_MANY_REQUESTS,
                &form.next,
                Some(&format!(
                    "Too many attempts. Try again in {} seconds.",
                    retry_after.as_secs().max(1)
                )),
            )),
            Err(AuthError::InvalidToken | AuthError::TokenExpired) => Ok(self.login_page(
                StatusCode::UNAUTHORIZED,
                &form.next,
                Some("Your sign-in expired. Sign in again."),
            )),
            Err(error) => Err(AppError::internal(error)),
        }
    }

    /// Starts the dashboard session: the login's refresh token is revoked
    /// at once (the dashboard keeps no API session), and the encrypted
    /// cookie is set.
    async fn signed_in(
        &self,
        auth: &AuthService,
        user_id: Uuid,
        refresh_token: &str,
        next: &str,
    ) -> Result<Response, AppError> {
        if let Err(error) = auth.logout(Some(refresh_token), user_id, false).await {
            tracing::warn!(target: "navree::lighthouse", %error, "could not revoke the sign-in's refresh token");
        }
        let sessions = self
            .sessions
            .as_ref()
            .ok_or_else(|| AppError::service_unavailable(UNAVAILABLE))?;
        let cookie = sessions
            .start(user_id)
            .ok_or_else(|| AppError::internal("could not encrypt the dashboard session"))?;
        let mut response = Redirect::to(&self.next_page(next)).into_response();
        let headers = response.headers_mut();
        headers.append(
            http::header::SET_COOKIE,
            HeaderValue::from_str(&cookie).map_err(AppError::internal)?,
        );
        Ok(response)
    }

    fn logout(&self) -> Result<Response, AppError> {
        let mut response = StatusCode::NO_CONTENT.into_response();
        if let Some(sessions) = &self.sessions {
            response.headers_mut().append(
                http::header::SET_COOKIE,
                HeaderValue::from_str(&sessions.end()).map_err(AppError::internal)?,
            );
        }
        Ok(response)
    }
}

const UNAVAILABLE: &str = "Signing in needs AuthModule and CryptoModule in the app. Use an access token with the JSON API instead.";
const EXPIRED: &str = "The sign-in form expired. Try again.";

/// `Nm ago` style age for a table: `now`, `6s`, `4 min`, `3 h`, `2 d`.
pub(crate) fn age_short(at: chrono::DateTime<chrono::Utc>) -> String {
    let seconds = (chrono::Utc::now() - at).num_seconds().max(0);
    match seconds {
        0..=4 => "now".to_owned(),
        5..=59 => format!("{seconds}s"),
        60..=3599 => format!("{} min", seconds / 60),
        3600..=86_399 => format!("{} h", seconds / 3600),
        _ => format!("{} d", seconds / 86_400),
    }
}

/// `18 min ago`, `3 h ago`: when something happened, in a sentence.
pub(crate) fn ago(at: chrono::DateTime<chrono::Utc>) -> String {
    let seconds = (chrono::Utc::now() - at).num_seconds();
    if seconds < 60 {
        return "just now".to_owned();
    }
    format!("{} ago", age_short(at))
}

/// Percent-encodes a query value.
fn encode(text: &str) -> String {
    form_urlencoded::byte_serialize(text.as_bytes()).collect()
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use navree_http::handler::Endpoint;

    use super::*;

    #[test]
    fn handlers_on_the_viewer_are_native() {
        async fn handler(_: Viewer) {}
        let _ = Endpoint::new(handler);
    }

    #[test]
    fn sign_in_forms_never_show_credentials_in_debug_output() {
        let login: LoginForm = serde_json::from_value(json!({
            "email": "ops@acme.test",
            "password": "lh-pa55word",
            "csrf": "lh-csrf-t0ken",
            "next": "/lighthouse",
        }))
        .unwrap();
        let two_factor: TwoFactorForm = serde_json::from_value(json!({
            "challenge": "lh-challenge-t0ken",
            "code": "123456",
            "csrf": "lh-csrf-t0ken",
            "next": "/lighthouse",
        }))
        .unwrap();
        let printed = format!("{login:?} {two_factor:?}");
        for secret in [
            "lh-pa55word",
            "lh-csrf-t0ken",
            "lh-challenge-t0ken",
            "123456",
        ] {
            assert!(!printed.contains(secret), "{secret} in {printed}");
        }
        assert!(printed.contains("ops@acme.test"), "{printed}");
    }
}
