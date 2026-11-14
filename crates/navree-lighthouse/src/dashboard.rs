//! The dashboard: server-rendered pages at `lighthouse.path`, drawn with
//! Navree UI in a shell of their own. Signing in is the app's: a browser
//! signs in on the shared session (navree-auth's `AuthPages`), and every
//! route here checks an ability on the subject `Lighthouse`.

use std::borrow::Cow;
use std::sync::Arc;

use http::request::Parts;
use http::{HeaderMap, HeaderValue, Method};
use navree_access::{Abilities, Subject as _};
use navree_auth::{AuthService, SIGN_OUT_PATH};
use navree_http::extract::FromRequestParts;
use navree_http::response::Response;
use navree_http::{AppError, ClientIp, Controller, CurrentPrincipal, Form, Path, Query, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree_session::Session;
use navree_ui::pages::region_answer;
use navree_ui::{
    Alert, AlertTone, AppPage, NavGroup, NavItem, Node, Reply, Shell, UserMenu, icons,
};
use uuid::Uuid;

use crate::api::{IssueFilterQuery, PageQuery, PeriodQuery, RouteQuery};
use crate::config_api::Configure;
use crate::module::{CONFIGURE, LighthouseSubject, RESOLVE, VIEW};
use crate::options::StoreKind;
use crate::pages::{ReleaseQuery, alerts, issues, lists, overview, releases, settings, traces};
use crate::service::Lighthouse;
use crate::store::LighthouseStore;

/// Who is looking at a page: the signed-in user, the session's CSRF token
/// (for the forms) and whether they may change issues and settings.
pub(crate) struct Viewer {
    pub(crate) user_id: Uuid,
    pub(crate) csrf: Option<String>,
    pub(crate) can_resolve: bool,
    pub(crate) can_configure: bool,
    /// The path and query of the request, for the shell.
    pub(crate) target: String,
    pub(crate) headers: HeaderMap,
}

impl FromRequestParts for Viewer {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        let CurrentPrincipal(principal) = CurrentPrincipal::from_parts(parts)?;
        let abilities = Abilities::from_request_parts(parts).await?;
        Ok(Viewer {
            user_id: principal.user_id,
            csrf: parts.extensions.get::<Session>().map(Session::csrf_token),
            can_resolve: abilities.can(RESOLVE, LighthouseSubject::SUBJECT, None),
            can_configure: abilities.can(CONFIGURE, LighthouseSubject::SUBJECT, None),
            target: parts
                .uri
                .path_and_query()
                .map_or_else(|| parts.uri.path().to_owned(), |target| target.to_string()),
            headers: parts.headers.clone(),
        })
    }
}

/// The dashboard's pages, mounted at `lighthouse.path` when
/// `lighthouse.dashboard` is on. Pages check `view` on the subject
/// `Lighthouse`; the buttons that change issues check `resolve`, and those
/// that change settings and alert channels check `configure`. A guest is
/// sent to the app's sign-in page, and a user without `view` gets a
/// forbidden page: both come from Navree UI's error pages, so the app needs
/// `UiModule` and `AuthPages` (with `SessionModule` and `AuthModule`) for
/// people to sign in.
pub struct LighthouseDashboardController {
    pub(crate) lighthouse: Arc<Lighthouse>,
    pub(crate) configure: Arc<Configure>,
    auth: Option<Arc<AuthService>>,
    /// `observe.tracing.trace_url`: the trace backend's page, with
    /// `{trace_id}`.
    trace_url: Option<String>,
}

impl Injectable for LighthouseDashboardController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<Lighthouse>(),
            ProviderKey::of::<Configure>(),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            let app = r.app();
            Ok(LighthouseDashboardController {
                auth: app.try_get::<AuthService>().await,
                trace_url: app
                    .try_get::<navree_config::Config>()
                    .await
                    .and_then(|config| config.get_string("observe.tracing.trace_url"))
                    .filter(|url| url.contains("{trace_id}")),
                lighthouse: r.get::<Lighthouse>().await?,
                configure: r.get::<Configure>().await?,
            })
        })
    }
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
        .page()
        .name($name)
    }};
}

/// A route that changes something: a form's post, checked for `ability`.
macro_rules! action {
    ($this:expr, $path:expr, $name:expr, $ability:expr, |$c:ident, $viewer:ident $(, $arg:ident : $ty:ty)*| $body:expr) => {{
        let this = $this.clone();
        RouteDef::on(Method::POST, $path, move |$viewer: Viewer $(, $arg: $ty)*| {
            let this = this.clone();
            async move {
                let $c: &LighthouseDashboardController = &this;
                let result: Result<Reply, AppError> = $body;
                result
            }
        })
        .can($ability, LighthouseSubject::SUBJECT, None)
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
        vec![
            page!(self, path(""), "lighthouse.overview", |c,
                                                          viewer,
                                                          q: Query<
                PeriodQuery,
            >| {
                c.overview(viewer, q.period()?).await
            }),
            page!(
                self,
                path("/overview/live"),
                "lighthouse.overview.live",
                |c, _viewer, q: Query<PeriodQuery>| { c.overview_live(q.period()?).await }
            ),
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
                |c, viewer, q: Query<ReleaseQuery>| { c.releases(viewer, q.0).await }
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
            action!(
                self,
                path("/issues/{id}/resolve"),
                "lighthouse.issues.resolve",
                RESOLVE,
                |c, _viewer, id: Path<Uuid>, form: Form<issues::ResolveForm>| {
                    c.resolve_issue(id.0, form.0).await
                }
            ),
            action!(
                self,
                path("/issues/{id}/ignore"),
                "lighthouse.issues.ignore",
                RESOLVE,
                |c, _viewer, id: Path<Uuid>, form: Form<issues::IgnoreForm>| {
                    c.ignore_issue(id.0, form.0).await
                }
            ),
            action!(
                self,
                path("/issues/{id}/reopen"),
                "lighthouse.issues.reopen",
                RESOLVE,
                |c, _viewer, id: Path<Uuid>| { c.reopen_issue(id.0).await }
            ),
            action!(
                self,
                path("/alerts/channels/{name}"),
                "lighthouse.alerts.channels.change",
                CONFIGURE,
                |c, viewer, name: Path<String>, ip: ClientIp, form: Form<Vec<(String, String)>>| {
                    c.change_channel(&viewer, ip, &name.0, form.0).await
                }
            ),
            action!(
                self,
                path("/settings"),
                "lighthouse.settings.change",
                CONFIGURE,
                |c, viewer, ip: ClientIp, form: Form<Vec<(String, String)>>| {
                    c.change_settings(&viewer, ip, form.0).await
                }
            ),
        ]
    }
}

impl LighthouseDashboardController {
    pub(crate) fn store(&self) -> Result<&LighthouseStore, AppError> {
        self.lighthouse
            .store()
            .ok_or_else(|| AppError::service_unavailable("lighthouse.store is off"))
    }

    /// The trace backend's page of `trace_id`, when `observe.tracing.trace_url`
    /// is set.
    pub(crate) fn trace_link(&self, trace_id: Option<&str>) -> Option<String> {
        Some(self.trace_url.as_ref()?.replace("{trace_id}", trace_id?))
    }

    pub(crate) fn mount(&self) -> &str {
        self.lighthouse.options().mount_path()
    }

    /// A page in the dashboard's shell: the navigation with the number of
    /// open issues and failed alerts, the environment and release under the
    /// name, and the signed-in user in the footer.
    pub(crate) async fn page(
        &self,
        viewer: &Viewer,
        title: impl Into<Cow<'static, str>>,
    ) -> Result<AppPage, AppError> {
        let store = self.store()?;
        let user = match &self.auth {
            Some(auth) => auth.find_user(viewer.user_id).await.ok().flatten(),
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
        let mount = self.mount().to_owned();
        let environment = self.lighthouse.options().environment_label();
        let context = match &release {
            Some(release) => format!("{environment} · {release}"),
            None => environment,
        };
        let (name, email) = match user {
            Some(user) => (user.name, user.email),
            None => (viewer.user_id.to_string(), String::new()),
        };
        let link = |label: &'static str, rest: &str, glyph| {
            NavItem::new(label, format!("{mount}{rest}")).icon(glyph)
        };
        let mut alert_item = link("Alerts", "/alerts", icons::BELL);
        if failed_alerts > 0 {
            alert_item = alert_item.count(failed_alerts.to_string());
        }
        let shell = Shell::new("Lighthouse")
            .context(context)
            .home(mount.clone())
            .group(
                NavGroup::new("Monitor")
                    .item(link("Overview", "", icons::LAYOUT_DASHBOARD))
                    .item(
                        link("Issues", "/issues", icons::TRIANGLE_ALERT)
                            .count(open_issues.to_string()),
                    )
                    .item(link("Requests", "/requests", icons::ACTIVITY))
                    .item(link("Queries", "/queries", icons::DATABASE))
                    .item(link("Jobs", "/jobs", icons::LIST_CHECKS))
                    .item(link("Outgoing", "/outgoing", icons::ARROW_UP_RIGHT)),
            )
            .group(
                NavGroup::new("Configure")
                    .item(alert_item)
                    .item(link("Releases", "/releases", icons::TAG))
                    .item(link("Settings", "/settings", icons::SETTINGS)),
            )
            .user(UserMenu::new(name, email).action("Sign out", SIGN_OUT_PATH));
        let app = shell.for_request(viewer.target.clone(), &viewer.headers);
        let mut page = app.page(title);
        let dropped = self.lighthouse.dropped();
        if dropped > 0 {
            page = page.body(
                Alert::new(AlertTone::Warning)
                    .title("Details are being dropped")
                    .body(format!(
                        "This process sampled out {dropped} traces and occurrences because the \
                         write queue was full: the database is not keeping up. Counts stay \
                         exact; only details are missing."
                    )),
            );
        }
        Ok(page)
    }

    async fn overview(&self, viewer: Viewer, period: crate::Period) -> Result<Response, AppError> {
        overview::page(self, viewer, period).await
    }

    async fn overview_live(&self, period: crate::Period) -> Result<Response, AppError> {
        let region = overview::live(self, period).await?;
        Ok(fragment(region.into()))
    }

    async fn issues(&self, viewer: Viewer, query: IssueFilterQuery) -> Result<Response, AppError> {
        issues::list(self, viewer, query).await
    }

    async fn issue(&self, viewer: Viewer, id: Uuid) -> Result<Response, AppError> {
        issues::detail(self, viewer, id).await
    }

    async fn requests(&self, viewer: Viewer, period: crate::Period) -> Result<Response, AppError> {
        traces::routes(self, viewer, period).await
    }

    async fn route(
        &self,
        viewer: Viewer,
        endpoint: &str,
        period: crate::Period,
    ) -> Result<Response, AppError> {
        traces::route(self, viewer, endpoint, period).await
    }

    async fn trace(&self, viewer: Viewer, id: Uuid) -> Result<Response, AppError> {
        traces::trace(self, viewer, id).await
    }

    async fn queries(&self, viewer: Viewer, period: crate::Period) -> Result<Response, AppError> {
        lists::queries(self, viewer, period).await
    }

    async fn jobs(&self, viewer: Viewer, period: crate::Period) -> Result<Response, AppError> {
        lists::jobs(self, viewer, period).await
    }

    async fn outgoing(&self, viewer: Viewer, period: crate::Period) -> Result<Response, AppError> {
        lists::outgoing(self, viewer, period).await
    }

    async fn alerts(&self, viewer: Viewer, query: PageQuery) -> Result<Response, AppError> {
        alerts::page(self, viewer, query).await
    }

    async fn releases(&self, viewer: Viewer, query: ReleaseQuery) -> Result<Response, AppError> {
        releases::page(self, viewer, query).await
    }

    async fn settings(&self, viewer: Viewer) -> Result<Response, AppError> {
        settings::page(self, viewer).await
    }

    async fn resolve_issue(&self, id: Uuid, form: issues::ResolveForm) -> Result<Reply, AppError> {
        issues::resolve(self, id, form).await
    }

    async fn ignore_issue(&self, id: Uuid, form: issues::IgnoreForm) -> Result<Reply, AppError> {
        issues::ignore(self, id, form).await
    }

    async fn reopen_issue(&self, id: Uuid) -> Result<Reply, AppError> {
        issues::reopen(self, id).await
    }

    async fn change_channel(
        &self,
        viewer: &Viewer,
        ip: ClientIp,
        name: &str,
        form: Vec<(String, String)>,
    ) -> Result<Reply, AppError> {
        alerts::change(self, viewer, ip, name, form).await
    }

    async fn change_settings(
        &self,
        viewer: &Viewer,
        ip: ClientIp,
        form: Vec<(String, String)>,
    ) -> Result<Reply, AppError> {
        settings::change(self, viewer, ip, form).await
    }
}

/// A part of a page, for a region to fetch: HTML with no script of its own, with what the
/// runtime needs to draw it.
fn fragment(body: Node) -> Response {
    let mut response = region_answer(body);
    response.headers_mut().insert(
        http::header::CACHE_CONTROL,
        HeaderValue::from_static("no-store"),
    );
    response
}

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
pub(crate) fn encode(text: &str) -> String {
    form_urlencoded::byte_serialize(text.as_bytes()).collect()
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
    fn ages_are_short_and_whole() {
        let ago_secs = |s: i64| age_short(chrono::Utc::now() - chrono::TimeDelta::seconds(s));
        assert_eq!(ago_secs(1), "now");
        assert_eq!(ago_secs(30), "30s");
        assert_eq!(ago_secs(300), "5 min");
        assert_eq!(ago_secs(7200), "2 h");
        assert_eq!(ago_secs(3 * 86_400), "3 d");
    }

    #[test]
    fn query_values_are_percent_encoded() {
        assert_eq!(encode("GET /api/{id}"), "GET+%2Fapi%2F%7Bid%7D");
    }
}
