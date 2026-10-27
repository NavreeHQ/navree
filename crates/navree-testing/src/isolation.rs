//! Isolation suites: cross-tenant tests generated from a module's route
//! table ([`TestApp::isolation_suite`]).
//!
//! A suite creates two tenants (workspaces, with navree-workspace's
//! `WorkspaceTenancy`) with one member who belongs to both, then sends every
//! route of the module from tenant A against records of tenant B:
//!
//! | Route | Expected |
//! |---|---|
//! | With path parameters (`GET`, `PATCH`, `PUT`, `POST`, `DELETE /deals/{id}`) | 404; the same `GET` answers 2xx in B (the control), and after a write B's record reads as before |
//! | `GET` without parameters (a list) | 2xx without B's record |
//! | `POST`, `PUT`, `PATCH`, `DELETE` without parameters, given a body | B's id in the body's owner field is ignored: the record is not created in B |
//!
//! Records come from the bindings: [`IsolationSuite::record`] names the path
//! parameter and creates a record of it in a tenant (navree-workspace's
//! `.factory(..)` binds `{id}` to a model factory).

use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::sync::Arc;

use http::{HeaderName, HeaderValue, Method, StatusCode};
use navree_http::{RouteMeta, route_table};
use navree_kernel::{AppContext, BoxError, BoxFuture, Module, ModuleId};
use serde_json::Value;
use uuid::Uuid;

use crate::{TestApp, TestClient, TestResponse, principal};

/// A tenant an isolation suite created, and how requests address it.
#[derive(Clone, Debug)]
pub struct Tenant {
    /// Its id (a workspace id).
    pub id: String,
    /// Its name.
    pub name: String,
    headers: Vec<(HeaderName, HeaderValue)>,
}

impl Tenant {
    /// Tenant `id` named `name`, addressed by no header yet.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            headers: Vec::new(),
        }
    }

    /// Adds a header every request to the tenant carries
    /// (`X-Workspace: <id>`).
    ///
    /// # Panics
    ///
    /// When `name` or `value` is not a valid header name or value.
    pub fn header(mut self, name: &str, value: &str) -> Self {
        let name = HeaderName::try_from(name)
            .unwrap_or_else(|error| panic!("invalid header name `{name}`: {error}"));
        let value = HeaderValue::try_from(value)
            .unwrap_or_else(|error| panic!("invalid value for header `{name}`: {error}"));
        self.headers.push((name, value));
        self
    }

    /// The headers that address requests to the tenant.
    pub fn headers(&self) -> &[(HeaderName, HeaderValue)] {
        &self.headers
    }
}

/// How an isolation suite creates tenants. navree-workspace's
/// `WorkspaceTenancy` creates workspaces addressed by the `X-Workspace`
/// header.
pub trait Tenancy: Send + Sync + 'static {
    /// Creates tenant `name` in which `member` may do everything.
    fn create<'a>(
        &'a self,
        app: &'a TestApp,
        name: &'a str,
        member: Uuid,
    ) -> BoxFuture<'a, Result<Tenant, BoxError>>;

    /// The field of a request body or record that names its tenant
    /// (`workspace_id`), which create routes must ignore; `None` skips that
    /// part of the check.
    fn owner_field(&self) -> Option<&str> {
        None
    }

    /// What reports call a tenant (`workspace`); default `tenant`.
    fn noun(&self) -> &str {
        "tenant"
    }
}

/// What a record binding gets: the app, the tenant to create the record in,
/// the path parameters bound before this one (outer resources first) and a
/// number unique within the suite.
#[derive(Clone, Debug)]
pub struct RecordContext {
    app: AppContext,
    tenant: Tenant,
    params: BTreeMap<String, String>,
    n: usize,
}

impl RecordContext {
    /// The app.
    pub fn app(&self) -> &AppContext {
        &self.app
    }

    /// The tenant the record belongs in.
    pub fn tenant(&self) -> &Tenant {
        &self.tenant
    }

    /// The ids bound to the route's earlier path parameters
    /// (`{deal}` before `{note}` in `/deals/{deal}/notes/{note}`).
    pub fn params(&self) -> &BTreeMap<String, String> {
        &self.params
    }

    /// A number unique within the suite run, for unique values.
    pub fn n(&self) -> usize {
        self.n
    }
}

type RecordFn =
    Arc<dyn Fn(RecordContext) -> BoxFuture<'static, Result<String, BoxError>> + Send + Sync>;

/// How one route fared.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IsolationOutcome {
    /// The route keeps tenants apart; what was checked.
    Passed(String),
    /// The route leaks, or could not be checked; why.
    Failed(String),
    /// Not checked; why.
    Skipped(String),
}

/// One route's result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteResult {
    /// `GET`.
    pub method: String,
    /// The mounted path template, `/api/v1/deals/{id}`.
    pub path: String,
    /// How it fared.
    pub outcome: IsolationOutcome,
}

/// Every route's result, in route table order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IsolationReport {
    /// The module tested.
    pub module: String,
    /// One result per route.
    pub routes: Vec<RouteResult>,
}

impl IsolationReport {
    fn count(&self, f: impl Fn(&IsolationOutcome) -> bool) -> usize {
        self.routes.iter().filter(|r| f(&r.outcome)).count()
    }

    /// Routes that passed.
    pub fn passed(&self) -> usize {
        self.count(|o| matches!(o, IsolationOutcome::Passed(_)))
    }

    /// Routes that failed.
    pub fn failed(&self) -> usize {
        self.count(|o| matches!(o, IsolationOutcome::Failed(_)))
    }

    /// Routes skipped.
    pub fn skipped(&self) -> usize {
        self.count(|o| matches!(o, IsolationOutcome::Skipped(_)))
    }

    /// Whether no route failed.
    pub fn is_ok(&self) -> bool {
        self.failed() == 0
    }
}

impl fmt::Display for IsolationReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "Isolation suite for {}: {} routes, {} passed, {} failed, {} skipped",
            self.module,
            self.routes.len(),
            self.passed(),
            self.failed(),
            self.skipped()
        )?;
        let width = self.routes.iter().map(|r| r.path.len()).max().unwrap_or(0);
        for route in &self.routes {
            let (label, detail) = match &route.outcome {
                IsolationOutcome::Passed(d) => ("ok", d),
                IsolationOutcome::Failed(d) => ("FAIL", d),
                IsolationOutcome::Skipped(d) => ("skip", d),
            };
            writeln!(
                f,
                "  {label:<6}{:<8}{:<width$}  {detail}",
                route.method, route.path
            )?;
        }
        Ok(())
    }
}

/// Cross-tenant tests for every route of one module; start one with
/// [`TestApp::isolation_suite`].
#[must_use = "an isolation suite does nothing until `run().await`"]
pub struct IsolationSuite<'a> {
    app: &'a TestApp,
    module: ModuleId,
    tenancy: Option<Arc<dyn Tenancy>>,
    records: Vec<(String, RecordFn)>,
    bodies: Vec<(String, String, Value)>,
    skips: Vec<(String, String, String)>,
}

impl TestApp {
    /// Cross-tenant tests for every route of module `M`: from one tenant,
    /// reading, changing or deleting another tenant's record must be a 404,
    /// lists must not show it and creates must not put records in it. Give
    /// the suite a tenancy and record bindings, then
    /// [`run`](IsolationSuite::run) it. With navree-workspace:
    ///
    /// ```ignore
    /// use navree::workspace::testing::IsolationSuiteExt;
    ///
    /// app.isolation_suite::<DealsModule>()
    ///     .factory(DealFactory::default()) // binds {id}; uses workspaces
    ///     .run()
    ///     .await;
    /// ```
    pub fn isolation_suite<M: Module>(&self) -> IsolationSuite<'_> {
        IsolationSuite {
            app: self,
            module: ModuleId::of::<M>(),
            tenancy: None,
            records: Vec::new(),
            bodies: Vec::new(),
            skips: Vec::new(),
        }
    }
}

impl IsolationSuite<'_> {
    /// Creates tenants with `tenancy`.
    pub fn tenancy(mut self, tenancy: impl Tenancy) -> Self {
        self.tenancy = Some(Arc::new(tenancy));
        self
    }

    /// Whether a tenancy is set.
    pub fn has_tenancy(&self) -> bool {
        self.tenancy.is_some()
    }

    /// Binds path parameter `param` (`id` for `{id}`): `create` makes a
    /// record in the tenant it is given and returns its id as the path
    /// shows it. A later binding of the same parameter replaces an earlier
    /// one.
    pub fn record<F, Fut>(mut self, param: &str, create: F) -> Self
    where
        F: Fn(RecordContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<String, BoxError>> + Send + 'static,
    {
        self.records.retain(|(name, _)| name != param);
        let create: RecordFn = Arc::new(move |cx| Box::pin(create(cx)));
        self.records.push((param.to_owned(), create));
        self
    }

    /// The JSON body to send to route `method path` (the mounted template,
    /// `"/api/v1/deals/{id}"`). Writes without one send `{}`; creates
    /// (no path parameters) without one are skipped.
    pub fn body(mut self, method: &str, path: &str, body: Value) -> Self {
        self.bodies
            .push((method.to_ascii_uppercase(), path.to_owned(), body));
        self
    }

    /// Leaves route `method path` out, reporting `reason`.
    pub fn skip(mut self, method: &str, path: &str, reason: &str) -> Self {
        self.skips.push((
            method.to_ascii_uppercase(),
            path.to_owned(),
            reason.to_owned(),
        ));
        self
    }

    /// Runs the suite and returns the report.
    ///
    /// # Panics
    ///
    /// With the report, when a route fails; when the suite cannot start (no
    /// tenancy, no routes, tenants cannot be created).
    pub async fn run(self) -> IsolationReport {
        let report = self
            .try_run()
            .await
            .unwrap_or_else(|error| panic!("the isolation suite could not run: {error}"));
        assert!(report.is_ok(), "{report}");
        report
    }

    /// Runs the suite; a failed route is in the report, not an error.
    ///
    /// # Errors
    ///
    /// When the suite cannot start: no tenancy, the module has no routes,
    /// or the tenants cannot be created.
    pub async fn try_run(self) -> Result<IsolationReport, BoxError> {
        let module_name = self.module.name();
        let tenancy = self.tenancy.clone().ok_or_else(|| {
            format!(
                "the isolation suite for {module_name} has no tenancy: call .tenancy(..), or .factory(..) from navree_workspace::testing, which uses workspaces"
            )
        })?;
        let all = route_table(self.app.ctx()).await?;
        let routes: Vec<RouteMeta> = all
            .into_iter()
            .filter(|route| route.module_id == Some(self.module))
            .collect();
        if routes.is_empty() {
            return Err(format!("{module_name} has no routes to test").into());
        }
        let member = Uuid::now_v7();
        let a = tenancy.create(self.app, "Isolation A", member).await?;
        let b = tenancy.create(self.app, "Isolation B", member).await?;
        let mut run = Run {
            suite: &self,
            tenancy: tenancy.as_ref(),
            client: self.app.acting_as(principal(member)),
            routes: &routes,
            a,
            b,
            counter: 0,
        };
        let mut results = Vec::new();
        for route in &routes {
            let outcome = run.route(route).await;
            results.push(RouteResult {
                method: route.method.clone(),
                path: route.full_path.clone(),
                outcome,
            });
        }
        Ok(IsolationReport {
            module: module_name.to_owned(),
            routes: results,
        })
    }

    fn body_for(&self, route: &RouteMeta) -> Option<&Value> {
        self.bodies
            .iter()
            .find(|(m, p, _)| *m == route.method && *p == route.full_path)
            .map(|(_, _, body)| body)
    }

    fn skip_reason(&self, route: &RouteMeta) -> Option<&str> {
        self.skips
            .iter()
            .find(|(m, p, _)| *m == route.method && *p == route.full_path)
            .map(|(_, _, reason)| reason.as_str())
    }

    fn binding(&self, param: &str) -> Option<&RecordFn> {
        self.records
            .iter()
            .find(|(name, _)| name == param)
            .map(|(_, f)| f)
    }
}

impl fmt::Debug for IsolationSuite<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IsolationSuite")
            .field("module", &self.module.name())
            .field("tenancy", &self.tenancy.is_some())
            .field(
                "records",
                &self.records.iter().map(|(p, _)| p).collect::<Vec<_>>(),
            )
            .finish_non_exhaustive()
    }
}

/// One run of a suite: the two tenants and the member's client.
struct Run<'s, 'a> {
    suite: &'s IsolationSuite<'a>,
    tenancy: &'s dyn Tenancy,
    client: TestClient,
    routes: &'s [RouteMeta],
    a: Tenant,
    b: Tenant,
    counter: usize,
}

impl Run<'_, '_> {
    async fn route(&mut self, route: &RouteMeta) -> IsolationOutcome {
        if let Some(reason) = self.suite.skip_reason(route) {
            return IsolationOutcome::Skipped(reason.to_owned());
        }
        let params = path_params(&route.full_path);
        if let Some(missing) = params.iter().find(|p| self.suite.binding(p).is_none()) {
            return IsolationOutcome::Failed(format!(
                "path parameter {{{missing}}} has no record: bind it with .record(\"{missing}\", ..), or skip the route"
            ));
        }
        let result = if !params.is_empty() {
            self.record_route(route, &params).await
        } else if route.method == "GET" || route.method == "HEAD" {
            self.list_route(route).await
        } else {
            self.create_route(route).await
        };
        result.unwrap_or_else(|error| {
            IsolationOutcome::Failed(format!("creating its records failed: {error}"))
        })
    }

    /// Creates records for `params` in `tenant`, outer ones first.
    async fn records(
        &mut self,
        params: &[String],
        tenant: &Tenant,
    ) -> Result<BTreeMap<String, String>, BoxError> {
        let mut bound = BTreeMap::new();
        for param in params {
            let Some(create) = self.suite.binding(param) else {
                continue;
            };
            self.counter += 1;
            let id = create(RecordContext {
                app: self.suite.app.ctx().clone(),
                tenant: tenant.clone(),
                params: bound.clone(),
                n: self.counter,
            })
            .await?;
            bound.insert(param.clone(), id);
        }
        Ok(bound)
    }

    async fn send(
        &self,
        method: &str,
        path: &str,
        tenant: &Tenant,
        body: Option<&Value>,
    ) -> TestResponse {
        let method = Method::from_bytes(method.as_bytes()).unwrap_or(Method::GET);
        let mut request = self.client.request(method, path);
        for (name, value) in tenant.headers() {
            request = request.header(name.clone(), value.clone());
        }
        if let Some(body) = body {
            request = request.json(body);
        }
        request.send().await
    }

    fn noun(&self) -> &str {
        self.tenancy.noun()
    }

    /// A route on one record: from A, B's record is a 404.
    async fn record_route(
        &mut self,
        route: &RouteMeta,
        params: &[String],
    ) -> Result<IsolationOutcome, BoxError> {
        let b = self.b.clone();
        let a = self.a.clone();
        let bound = self.records(params, &b).await?;
        let path = substitute(&route.full_path, &bound);
        let method = route.method.as_str();
        let read = method == "GET" || method == "HEAD";
        let show = self
            .routes
            .iter()
            .any(|r| r.method == "GET" && r.full_path == route.full_path);
        let noun = self.noun().to_owned();
        // The control: the record answers in its own tenant.
        let control_method = if read { method } else { "GET" };
        let before = if read || show {
            let before = self.send(control_method, &path, &b, None).await;
            if !before.status().is_success() {
                return Ok(IsolationOutcome::Failed(format!(
                    "the control request failed: {control_method} in the record's own {noun} answered {}, so the suite cannot tell isolation from a broken route",
                    before.status().as_u16()
                )));
            }
            Some(before)
        } else {
            None
        };
        let empty = Value::Object(serde_json::Map::new());
        let body = match self.suite.body_for(route) {
            Some(body) => Some(body),
            None if read || method == "DELETE" => None,
            None => Some(&empty),
        };
        let cross = self.send(method, &path, &a, body).await;
        let status = cross.status();
        if status == StatusCode::NOT_FOUND {
            if read {
                let own = before.as_ref().map_or(0, |b| b.status().as_u16());
                return Ok(IsolationOutcome::Passed(format!(
                    "404 from another {noun}, {own} in its own"
                )));
            }
            let Some(before) = before else {
                return Ok(IsolationOutcome::Passed(format!("404 from another {noun}")));
            };
            let after = self.send("GET", &path, &b, None).await;
            return Ok(
                if after.status() == before.status() && after.text() == before.text() {
                    IsolationOutcome::Passed(format!(
                        "404 from another {noun}; the record is unchanged"
                    ))
                } else {
                    IsolationOutcome::Failed(format!(
                        "answered 404 to another {noun} but changed the record"
                    ))
                },
            );
        }
        Ok(IsolationOutcome::Failed(refusal(
            status,
            &cross,
            &noun,
            match method {
                "GET" | "HEAD" => "read",
                "DELETE" => "deleted",
                _ => "changed",
            },
            route,
        )))
    }

    /// A list: from A, B's record is not in it.
    async fn list_route(&mut self, route: &RouteMeta) -> Result<IsolationOutcome, BoxError> {
        let noun = self.noun().to_owned();
        let Some((param, _)) = self.suite.records.first() else {
            return Ok(IsolationOutcome::Skipped(
                "no record to look for: bind one with .record(..)".to_owned(),
            ));
        };
        let param = param.clone();
        let b = self.b.clone();
        let a = self.a.clone();
        let id = self
            .records(std::slice::from_ref(&param), &b)
            .await?
            .remove(&param)
            .unwrap_or_default();
        let cross = self.send(&route.method, &route.full_path, &a, None).await;
        if !cross.status().is_success() {
            return Ok(IsolationOutcome::Failed(format!(
                "answered {} to a member of the {noun}; a list must answer 2xx",
                cross.status().as_u16()
            )));
        }
        if mentions(&cross, &id) {
            return Ok(IsolationOutcome::Failed(format!(
                "another {noun}'s record is listed ({})",
                cross.status().as_u16()
            )));
        }
        let own = self.send(&route.method, &route.full_path, &b, None).await;
        Ok(IsolationOutcome::Passed(if mentions(&own, &id) {
            format!("another {noun}'s records are not listed")
        } else {
            format!(
                "another {noun}'s records are not listed (its own {noun}'s list does not show the record either)"
            )
        }))
    }

    /// A create (or another write without a record): B's id in the body's
    /// owner field does not put the record in B.
    async fn create_route(&mut self, route: &RouteMeta) -> Result<IsolationOutcome, BoxError> {
        let noun = self.noun().to_owned();
        let Some(body) = self.suite.body_for(route) else {
            return Ok(IsolationOutcome::Skipped(format!(
                "no request body: give one with .body(\"{}\", \"{}\", json)",
                route.method, route.full_path
            )));
        };
        let mut body = body.clone();
        let field = self.tenancy.owner_field().map(str::to_owned);
        if let (Some(field), Value::Object(map)) = (&field, &mut body) {
            map.insert(field.clone(), Value::String(self.b.id.clone()));
        }
        let a = self.a.clone();
        let b = self.b.clone();
        let cross = self
            .send(&route.method, &route.full_path, &a, Some(&body))
            .await;
        let status = cross.status();
        if status.is_client_error() && status != StatusCode::UNAUTHORIZED {
            return Ok(IsolationOutcome::Passed(format!(
                "answered {}: the request naming another {noun} was refused",
                status.as_u16()
            )));
        }
        if !status.is_success() {
            return Ok(IsolationOutcome::Failed(refusal(
                status, &cross, &noun, "changed", route,
            )));
        }
        let created = cross.json_value_or_null();
        if let Some(field) = &field
            && created.get(field).and_then(Value::as_str) == Some(b.id.as_str())
        {
            return Ok(IsolationOutcome::Failed(format!(
                "answered {} and put the record in another {noun} ({field})",
                status.as_u16()
            )));
        }
        // When the module lists records at the same path, B's list must not
        // show the new one.
        if let Some(id) = created.get("id").map(id_text)
            && self
                .routes
                .iter()
                .any(|r| r.method == "GET" && r.full_path == route.full_path)
        {
            let list = self.send("GET", &route.full_path, &b, None).await;
            if mentions(&list, &id) {
                return Ok(IsolationOutcome::Failed(format!(
                    "answered {} and the record shows in another {noun}'s list",
                    status.as_u16()
                )));
            }
        }
        Ok(IsolationOutcome::Passed(format!(
            "{}; the record stays in the caller's {noun}",
            status.as_u16()
        )))
    }
}

/// Why a cross-tenant answer other than 404 fails.
fn refusal(
    status: StatusCode,
    response: &TestResponse,
    noun: &str,
    verb: &str,
    route: &RouteMeta,
) -> String {
    let code = status.as_u16();
    match code {
        200..=399 => format!("answered {code} to another {noun}: its record was {verb}"),
        401 => "answered 401: the suite's requests are not authenticated; build the app with .fake_auth()".to_owned(),
        403 => format!(
            "answered 403 to another {noun}: a record of another {noun} must be a 404, so its existence does not show"
        ),
        400 | 415 | 422 => format!(
            "answered {code} before the record was looked up: give the route a valid body with .body(\"{}\", \"{}\", json)",
            route.method, route.full_path
        ),
        _ => {
            let mut text = response.text();
            if text.len() > 200 {
                text.truncate(200);
            }
            format!("answered {code} to another {noun}, expected 404: {text}")
        }
    }
}

/// The parameter names of a path template: `{id}` and `{*rest}`.
fn path_params(template: &str) -> Vec<String> {
    let mut params = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        let Some(end) = rest[start..].find('}') else {
            break;
        };
        let name = rest[start + 1..start + end].trim_start_matches('*');
        params.push(name.to_owned());
        rest = &rest[start + end + 1..];
    }
    params
}

/// `template` with each `{param}` replaced by its bound value.
fn substitute(template: &str, values: &BTreeMap<String, String>) -> String {
    let mut path = template.to_owned();
    for (name, value) in values {
        path = path
            .replace(&format!("{{{name}}}"), value)
            .replace(&format!("{{*{name}}}"), value);
    }
    path
}

/// A JSON id as text (`"7"` for 7).
fn id_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Whether `response` shows record `id`: an `id` member equal to it
/// anywhere in a JSON body, or (ids of eight characters or more, such as
/// UUIDs) the id anywhere in the text.
fn mentions(response: &TestResponse, id: &str) -> bool {
    fn walk(value: &Value, id: &str) -> bool {
        match value {
            Value::Object(map) => {
                map.get("id").is_some_and(|v| id_text(v) == id) || map.values().any(|v| walk(v, id))
            }
            Value::Array(items) => items.iter().any(|v| walk(v, id)),
            _ => false,
        }
    }
    if id.is_empty() {
        return false;
    }
    if id.len() >= 8 && response.text().contains(id) {
        return true;
    }
    walk(&response.json_value_or_null(), id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_parameters_are_read_in_order() {
        assert_eq!(
            path_params("/api/v1/deals/{deal}/notes/{note}"),
            ["deal", "note"]
        );
        assert_eq!(path_params("/files/{*path}"), ["path"]);
        assert!(path_params("/api/v1/deals").is_empty());
    }

    #[test]
    fn substitute_fills_every_parameter() {
        let values = BTreeMap::from([
            ("deal".to_owned(), "1".to_owned()),
            ("path".to_owned(), "a/b".to_owned()),
        ]);
        assert_eq!(
            substitute("/deals/{deal}/files/{*path}", &values),
            "/deals/1/files/a/b"
        );
    }
}
