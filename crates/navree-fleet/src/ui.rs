//! The dashboard's pages, drawn with Navree UI and served at `fleet.path`
//! (`/_fleet`), next to the JSON API under `{path}/api`.
//!
//! | Route | Page |
//! |---|---|
//! | `GET {path}` | the overview: figures, the workload of each supervisor, jobs per minute, the newest failed jobs |
//! | `GET {path}/supervisors` | each master, its supervisors and their processes |
//! | `GET {path}/jobs?tab=pending\|completed&search=`, `GET {path}/jobs/{id}` | pending and completed jobs, and one pending job |
//! | `GET {path}/failed?search=`, `GET {path}/failed/{id}` | failed jobs, and one with its error and payload |
//! | `GET {path}/metrics?period=hour\|day\|week` | throughput, runtime and wait charts |
//! | `POST {path}/pause`, `POST {path}/continue` | pause or continue one supervisor (`supervisor`) or all |
//! | `POST {path}/failed/{id}/retry`, `POST {path}/failed/{id}/forget`, `POST {path}/failed/retry` | handle failed jobs |
//!
//! The pages need no script: every action is a form that posts with the
//! session's CSRF token and comes back to the page it was sent from. They
//! sign in on the app's browser session like every other Navree UI page: a
//! browser without one reaches the app's sign-in page and comes back, and
//! the abilities are the API's (`fleet.view` to read, `fleet.operate` to
//! pause, continue, retry and forget). The parts that change redraw
//! themselves every few seconds while the page is open.

mod actions;
mod failed;
mod format;
mod jobs;
mod metrics;
mod overview;
mod parts;
mod supervisors;

use std::sync::Arc;

use chrono::{DateTime, Utc};
use http::Method;
use http::request::Parts;
use navree_access::Subject;
use navree_auth::{CurrentUser, SIGN_OUT_PATH};
use navree_http::extract::FromRequestParts;
use navree_http::response::{IntoResponse, Response};
use navree_http::{AppError, Form, Path, Query, RouteDef};
use navree_session::Session;
use navree_ui::{AppPage, AppShell, NavGroup, NavItem, Shell, UserMenu, icons};

use crate::dashboard::{FleetSubject, OPERATE, VIEW, api_error};
use crate::{Fleet, Overview};

/// What every page of the dashboard knows when it is drawn.
pub(crate) struct Frame {
    /// The dashboard's path (`/_fleet`).
    pub(crate) base: String,
    /// This page's address with its query: what a region that redraws itself
    /// fetches, and where a form sends the user back to.
    pub(crate) here: String,
    /// The session's CSRF token, which every form carries.
    pub(crate) csrf: Option<String>,
    /// Now, on Fleet's clock.
    pub(crate) now: DateTime<Utc>,
    /// The overview: the shell reads the status and the counts from it.
    pub(crate) overview: Overview,
    app: AppShell,
}

impl Frame {
    /// The frame of the request in `parts`.
    pub(crate) async fn load(fleet: &Fleet, parts: &mut Parts) -> Result<Frame, AppError> {
        let overview = fleet.overview().await.map_err(api_error)?;
        let user = user_of(parts).await;
        let base = fleet.options().base_path().to_owned();
        let path = parts.uri.path().to_owned();
        let here = parts
            .uri
            .path_and_query()
            .map_or_else(|| path.clone(), |target| target.as_str().to_owned());
        let csrf = parts.extensions.get::<Session>().map(Session::csrf_token);
        let app = shell(&base, &overview, user).for_request(here.clone(), &parts.headers);
        Ok(Frame {
            base,
            here,
            csrf,
            now: fleet.now(),
            overview,
            app,
        })
    }

    /// A page titled `title`, in the shell.
    pub(crate) fn page(&self, title: impl Into<String>) -> AppPage {
        self.app.page(title.into())
    }
}

/// The signed-in user's name and email, when they can be read.
async fn user_of(parts: &mut Parts) -> Option<(String, String)> {
    let CurrentUser(user) = CurrentUser::from_request_parts(parts).await.ok()?;
    Some((user.name.clone(), user.email.clone()))
}

/// The shell of the tool: the pages down the side, the failed count and the
/// number of processes beside their names, and the user's menu.
fn shell(base: &str, overview: &Overview, user: Option<(String, String)>) -> Shell {
    let processes: usize = overview.supervisors.iter().map(|s| s.processes).sum();
    let mut supervisors =
        NavItem::new("Supervisors", format!("{base}/supervisors")).icon(icons::SERVER);
    if processes > 0 {
        supervisors = supervisors.count(format::number(processes as u64));
    }
    let mut failed =
        NavItem::new("Failed jobs", format!("{base}/failed")).icon(icons::TRIANGLE_ALERT);
    if overview.failed_total > 0 {
        failed = failed.count(format::number(overview.failed_total));
    }
    let monitor = NavGroup::new("Monitor")
        .item(NavItem::new("Overview", base.to_owned()).icon(icons::LAYOUT_DASHBOARD))
        .item(supervisors)
        .item(NavItem::new("Jobs", format!("{base}/jobs")).icon(icons::LIST_CHECKS))
        .item(failed)
        .item(NavItem::new("Metrics", format!("{base}/metrics")).icon(icons::CHART_COLUMN));
    let mut shell = Shell::new("Fleet")
        .context(format!("Queue workers \u{b7} {}", format::status(overview.status)))
        .home(base.to_owned())
        .group(monitor);
    if let Some((name, email)) = user {
        shell = shell.user(UserMenu::new(name, email).action("Sign out", SIGN_OUT_PATH));
    }
    shell
}

/// A page of the dashboard: readable with the `view` ability, and drawn as a
/// page (not as JSON) when it fails.
fn readable(route: RouteDef) -> RouteDef {
    route.can(VIEW, FleetSubject::SUBJECT, None).page()
}

/// An action of the dashboard: needs the `operate` ability.
fn operable(route: RouteDef) -> RouteDef {
    route.can(OPERATE, FleetSubject::SUBJECT, None).page()
}

/// The routes of the dashboard's pages and actions under `base`.
pub(crate) fn routes(fleet: &Arc<Fleet>, base: &str) -> Vec<RouteDef> {
    let at = |suffix: &str| format!("!{base}{suffix}");
    let mut routes = Vec::new();

    let f = fleet.clone();
    routes.push(
        readable(RouteDef::on(
            Method::GET,
            at(""),
            move |Query(query): Query<overview::Params>, mut parts: Parts| {
                let fleet = f.clone();
                async move { respond(overview::page(&fleet, &query, &mut parts).await) }
            },
        ))
        .name("fleet.dashboard"),
    );

    let f = fleet.clone();
    routes.push(
        readable(RouteDef::on(
            Method::GET,
            at("/supervisors"),
            move |mut parts: Parts| {
                let fleet = f.clone();
                async move { respond(supervisors::page(&fleet, &mut parts).await) }
            },
        ))
        .name("fleet.supervisors.page"),
    );

    let f = fleet.clone();
    routes.push(
        readable(RouteDef::on(
            Method::GET,
            at("/jobs"),
            move |Query(query): Query<jobs::Params>, mut parts: Parts| {
                let fleet = f.clone();
                async move { respond(jobs::list(&fleet, &query, &mut parts).await) }
            },
        ))
        .name("fleet.jobs.page"),
    );

    let f = fleet.clone();
    routes.push(
        readable(RouteDef::on(
            Method::GET,
            at("/jobs/{id}"),
            move |Path(id): Path<String>, mut parts: Parts| {
                let fleet = f.clone();
                async move { respond(jobs::show(&fleet, &id, &mut parts).await) }
            },
        ))
        .name("fleet.jobs.show.page"),
    );

    let f = fleet.clone();
    routes.push(
        readable(RouteDef::on(
            Method::GET,
            at("/failed"),
            move |Query(query): Query<failed::Params>, mut parts: Parts| {
                let fleet = f.clone();
                async move { respond(failed::list(&fleet, &query, &mut parts).await) }
            },
        ))
        .name("fleet.failed.page"),
    );

    let f = fleet.clone();
    routes.push(
        readable(RouteDef::on(
            Method::GET,
            at("/failed/{id}"),
            move |Path(id): Path<String>, mut parts: Parts| {
                let fleet = f.clone();
                async move { respond(failed::show(&fleet, &id, &mut parts).await) }
            },
        ))
        .name("fleet.failed.show.page"),
    );

    let f = fleet.clone();
    routes.push(
        readable(RouteDef::on(
            Method::GET,
            at("/metrics"),
            move |Query(query): Query<metrics::Params>, mut parts: Parts| {
                let fleet = f.clone();
                async move { respond(metrics::page(&fleet, &query, &mut parts).await) }
            },
        ))
        .name("fleet.metrics.page"),
    );

    for (action, pause) in [("pause", true), ("continue", false)] {
        let f = fleet.clone();
        let base = base.to_owned();
        routes.push(
            operable(RouteDef::on(
                Method::POST,
                at(&format!("/{action}")),
                move |Form(form): Form<actions::Params>| {
                    let fleet = f.clone();
                    let base = base.clone();
                    async move { actions::supervisors(&fleet, &base, &form, pause).await }
                },
            ))
            .name(&format!("fleet.{action}.page")),
        );
    }

    let f = fleet.clone();
    let b = base.to_owned();
    routes.push(
        operable(RouteDef::on(
            Method::POST,
            at("/failed/retry"),
            move |Form(form): Form<actions::Params>| {
                let fleet = f.clone();
                let base = b.clone();
                async move { actions::retry_all(&fleet, &base, &form).await }
            },
        ))
        .name("fleet.failed.retry_all.page"),
    );

    let f = fleet.clone();
    let b = base.to_owned();
    routes.push(
        operable(RouteDef::on(
            Method::POST,
            at("/failed/{id}/retry"),
            move |Path(id): Path<String>, Form(form): Form<actions::Params>| {
                let fleet = f.clone();
                let base = b.clone();
                async move { actions::retry(&fleet, &base, &id, &form).await }
            },
        ))
        .name("fleet.failed.retry.page"),
    );

    let f = fleet.clone();
    let b = base.to_owned();
    routes.push(
        operable(RouteDef::on(
            Method::POST,
            at("/failed/{id}/forget"),
            move |Path(id): Path<String>, Form(form): Form<actions::Params>| {
                let fleet = f.clone();
                let base = b.clone();
                async move { actions::forget(&fleet, &base, &id, &form).await }
            },
        ))
        .name("fleet.failed.forget.page"),
    );

    routes
}

/// A page as a response, or the error as one.
fn respond(page: Result<AppPage, AppError>) -> Result<Response, AppError> {
    page.map(IntoResponse::into_response)
}
