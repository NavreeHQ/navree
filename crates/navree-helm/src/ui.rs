//! The panel's server-rendered pages.

use std::collections::{BTreeMap, HashMap};
use std::error::Error as StdError;
use std::sync::Arc;

use http::header::{CACHE_CONTROL, CONTENT_TYPE, SET_COOKIE};
use http::{HeaderMap, HeaderValue, StatusCode};
use navree_access::{CREATE, READ};
use navree_auth::{AuthError, AuthService, ClientMeta, LoginOutcome, TwoFactorProof};
use navree_contracts::{AuthState, AuthVia};
use navree_crypto::{Encrypter, constant_time_eq, random_token};
use navree_http::body::{Bytes, to_bytes};
use navree_http::extract::{FromRequest, FromRequestParts, Multipart, Request};
use navree_http::response::{IntoResponse, Response};
use navree_http::{AppError, Path, RawQuery};
use navree_kernel::AppContext;
use navree_storage::{Storage, UploadRules, store_upload};
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::api::{parse_range, resource};
use crate::core::{CSS, Core, JS};
use crate::cx::Cx;
use crate::field::{Field, FieldKind};
use crate::handle::{ActionInfo, ErasedResource, RawRecord, ResourceInfo};
use crate::metric::MetricResult;
use crate::panel::Panel;
use crate::present::{Item, Screen, encode, present, screen_fields};
use crate::query::ListQuery;
use crate::session::{
    FLASH_COOKIE, LOGIN_COOKIE, PanelSession, SESSION_COOKIE, SIDEBAR_COOKIE, SessionData,
    build_cookie, clear_cookie, cookie, redirect, request_host, session_cookie,
};
use crate::values::key_text;
use crate::workspace::{WORKSPACE_COOKIE, requested};
use crate::{ActionResult, icons, money};

/// The Settings screens (feature `settings`).
#[cfg(feature = "settings")]
pub(crate) mod settings;

// ---------------------------------------------------------------------------
// Responses
// ---------------------------------------------------------------------------

/// An HTML response.
fn html(status: StatusCode, body: String) -> Response {
    let mut response = (status, body).into_response();
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    response
}

/// `template` rendered with `ctx`, or a 500 page.
fn render(core: &Core, status: StatusCode, template: &str, ctx: Value) -> Response {
    match core.views.render(template, ctx) {
        Ok(body) => html(status, body),
        Err(error) => {
            tracing::error!(target: "navree::helm", %error, template, "a panel template failed to render");
            html(
                StatusCode::INTERNAL_SERVER_ERROR,
                "<!doctype html><title>Error</title><p>The page failed to render.</p>".to_owned(),
            )
        }
    }
}

/// The `panel` object of every template.
fn panel_json(core: &Core, csrf: &str) -> Value {
    json!({
        "title": core.options.title,
        "logo": core.options.logo,
        "base": core.options.base(),
        "version": core.assets.version,
        "csrf": csrf,
        "collapsed": false,
        "icons": icons::ui(),
    })
}

/// A standalone error page.
pub(crate) fn status_page(core: &Core, status: StatusCode, message: Option<String>) -> Response {
    let reason = status.canonical_reason().unwrap_or("Error");
    let message = message.unwrap_or_else(|| default_message(status).to_owned());
    render(
        core,
        status,
        "helm/error.html",
        json!({
            "panel": panel_json(core, ""),
            "status": status.as_u16(),
            "reason": reason,
            "message": message,
        }),
    )
}

fn default_message(status: StatusCode) -> &'static str {
    match status.as_u16() {
        403 => "You may not open this page.",
        404 => "This page does not exist, or you may not see it.",
        419 | 400 => "The request was not valid.",
        429 => "Too many requests. Wait a moment and try again.",
        _ => "Something went wrong. Try again in a moment.",
    }
}

/// An error from the panel's own code as a page: database errors mapped
/// like the API maps them, server errors logged.
pub(crate) fn page_error(core: &Core, error: AppError) -> Response {
    let error = map_db(error);
    if error.status.is_server_error() {
        tracing::error!(
            target: "navree::helm",
            status = error.status.as_u16(),
            title = %error.title,
            detail = error.detail.as_deref().unwrap_or(""),
            source = error.source.as_ref().map(|s| s.to_string()).unwrap_or_default(),
            "a panel page failed"
        );
        return status_page(core, error.status, None);
    }
    let message = match &error.errors {
        Some(errors) if !errors.is_empty() => Some(
            errors
                .values()
                .flatten()
                .cloned()
                .collect::<Vec<_>>()
                .join(" "),
        ),
        _ => error.detail.clone(),
    };
    let mut page = status_page(core, error.status, message);
    for (name, value) in error.headers.iter() {
        page.headers_mut().insert(name.clone(), value.clone());
    }
    page
}

/// A 500 caused by a database error becomes the status the API gives it
/// (404 for a missing record, 409 for a conflict).
fn map_db(error: AppError) -> AppError {
    if error.status != StatusCode::INTERNAL_SERVER_ERROR {
        return error;
    }
    let mut next: Option<&(dyn StdError + 'static)> = error.source.as_deref().map(|e| e as _);
    while let Some(cause) = next {
        if let Some(db) = cause.downcast_ref::<navree_db::DbError>()
            && let Some(mapped) = navree_db::map_db_error(db)
        {
            return mapped;
        }
        next = cause.source();
    }
    error
}

// ---------------------------------------------------------------------------
// Flash messages
// ---------------------------------------------------------------------------

/// A one-time message.
#[derive(Clone, Debug, Serialize)]
struct Flash {
    text: String,
    tone: &'static str,
}

/// The flash message of the request, if any.
fn read_flash(headers: &HeaderMap) -> Option<Flash> {
    let value = cookie(headers, FLASH_COOKIE)?;
    let (tone, text) = value.split_once('|')?;
    let tone = match tone {
        "error" => "error",
        "warning" => "warning",
        _ => "success",
    };
    (!text.is_empty()).then(|| Flash {
        text: text.chars().take(500).collect(),
        tone,
    })
}

/// A redirect carrying a flash message.
fn redirect_with(location: &str, text: &str, tone: &str, core: &Core, host: &str) -> Response {
    let mut response = redirect(location);
    let value = format!("{tone}|{text}");
    if let Some(header) = build_cookie(FLASH_COOKIE, value, core.options.base(), 60, host) {
        response.headers_mut().append(SET_COOKIE, header);
    }
    response
}

/// Clears the flash cookie once it was shown.
fn clear_flash(response: &mut Response, core: &Core, host: &str) {
    if let Some(header) = clear_cookie(FLASH_COOKIE, core.options.base(), host) {
        response.headers_mut().append(SET_COOKIE, header);
    }
}

// ---------------------------------------------------------------------------
// The layout
// ---------------------------------------------------------------------------

/// A breadcrumb.
#[derive(Clone, Debug, Serialize)]
struct Crumb {
    label: String,
    url: Option<String>,
}

fn crumb(label: impl Into<String>, url: Option<String>) -> Crumb {
    Crumb {
        label: label.into(),
        url,
    }
}

/// What a page shows around its content.
struct Frame<'a> {
    cx: &'a Cx,
    session: &'a PanelSession,
    headers: &'a HeaderMap,
    host: String,
}

impl<'a> Frame<'a> {
    fn new(panel: &'a Panel, headers: &'a HeaderMap) -> Self {
        let host = headers
            .get(http::header::HOST)
            .and_then(|v| v.to_str().ok())
            .map(|h| h.rsplit_once(':').map_or(h, |(h, _)| h).to_owned())
            .unwrap_or_default();
        Frame {
            cx: &panel.cx,
            session: &panel.session,
            headers,
            host,
        }
    }

    fn core(&self) -> &Core {
        &self.cx.core
    }

    fn base(&self) -> &str {
        self.cx.core.options.base()
    }

    fn csrf(&self) -> &str {
        self.session.csrf.as_deref().unwrap_or("")
    }

    /// The template context of the layout plus `extra`.
    async fn context(&self, title: &str, active: &str, crumbs: Vec<Crumb>, extra: Value) -> Value {
        let cx = self.cx;
        let core = self.core();
        let base = self.base();
        let mut groups: Vec<(String, Vec<Value>)> = Vec::new();
        let mut push = |group: &str, item: Value| match groups.iter_mut().find(|(g, _)| g == group)
        {
            Some((_, items)) => items.push(item),
            None => groups.push((group.to_owned(), vec![item])),
        };
        for resource in core.registry.resources() {
            let info = resource.info();
            if !cx.can(READ, info.subject, None) {
                continue;
            }
            push(
                &info.group,
                json!({
                    "label": info.label,
                    "url": format!("{base}/resources/{}", info.slug),
                    "active": active == format!("resource:{}", info.slug),
                    "icon": icons::svg(info.icon),
                }),
            );
        }
        for entry in core.registry.pages() {
            let page = &entry.page;
            if page.can.as_ref().is_some_and(|(a, s)| !cx.can(a, s, None)) {
                continue;
            }
            push(
                page.group.as_deref().unwrap_or("Pages"),
                json!({
                    "label": page.title,
                    "url": format!("{base}/pages/{}", page.slug),
                    "active": active == format!("page:{}", page.slug),
                    "icon": icons::svg(&page.icon),
                }),
            );
        }
        #[cfg(feature = "settings")]
        if settings::visible(cx).await {
            push(
                "System",
                json!({
                    "label": "Settings",
                    "url": format!("{base}/settings"),
                    "active": active == "settings",
                    "icon": icons::svg("sliders-horizontal"),
                }),
            );
        }
        let menu: Vec<Value> = groups
            .into_iter()
            .map(|(title, items)| json!({ "title": title, "items": items }))
            .collect();
        let user = user_json(cx).await;
        let avatar = initials(user["name"].as_str().unwrap_or_default());
        let works_in_workspaces = core.registry.resources().iter().any(|resource| {
            let info = resource.info();
            info.workspaces.is_some() && cx.can(READ, info.subject, None)
        });
        let mut panel = panel_json(core, self.csrf());
        panel["collapsed"] =
            Value::Bool(cookie(self.headers, SIDEBAR_COOKIE).as_deref() == Some("collapsed"));
        let mut ctx = json!({
            "panel": panel,
            "page_title": title,
            "active": active,
            "menu": menu,
            "crumbs": crumbs,
            "user": user,
            "avatar": avatar,
            "flash": read_flash(self.headers),
            "workspace": {
                "shown": works_in_workspaces,
                "current": cx.uow.workspace().map(|id| id.to_string()),
            },
        });
        if let (Value::Object(ctx), Value::Object(extra)) = (&mut ctx, extra) {
            ctx.extend(extra);
        }
        ctx
    }

    /// Renders a page in the layout, clearing a shown flash message.
    async fn page(
        &self,
        status: StatusCode,
        template: &str,
        title: &str,
        active: &str,
        crumbs: Vec<Crumb>,
        extra: Value,
    ) -> Response {
        let ctx = self.context(title, active, crumbs, extra).await;
        let flashed = !ctx["flash"].is_null();
        let mut response = render(self.core(), status, template, ctx);
        if flashed {
            clear_flash(&mut response, self.core(), &self.host);
        }
        response
    }
}

/// The initials of a name for the user's avatar: the first letter of its
/// first and last words (`Maya Manager` → `MM`).
fn initials(name: &str) -> String {
    let first = |word: &str| word.chars().find(|c| c.is_alphanumeric());
    let mut words = name.split_whitespace();
    let mut out = String::new();
    if let Some(c) = words.next().and_then(first) {
        out.extend(c.to_uppercase());
    }
    if let Some(c) = words.next_back().and_then(first) {
        out.extend(c.to_uppercase());
    }
    out
}

/// `{id, name, email}` of the signed-in user.
async fn user_json(cx: &Cx) -> Value {
    if let Some(auth) = cx.app.try_get::<AuthService>().await
        && let Ok(Some(user)) = auth.find_user(cx.user_id).await
    {
        return json!({ "id": user.id, "name": user.name, "email": user.email });
    }
    json!({ "id": cx.user_id, "name": "Signed in", "email": "" })
}

// ---------------------------------------------------------------------------
// Assets
// ---------------------------------------------------------------------------

/// `GET {path}/assets/{file}`: the stylesheet, the theme and the script,
/// cached for a year (their URLs carry a version).
pub(crate) async fn asset(Path(file): Path<String>, request: Request) -> Response {
    let Some(app) = request.extensions().get::<AppContext>().cloned() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(core) = app.get::<Core>().await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let (body, content_type) = match file.as_str() {
        "helm.css" => (CSS.to_owned(), "text/css; charset=utf-8"),
        "theme.css" => (core.assets.theme.clone(), "text/css; charset=utf-8"),
        "helm.js" => (JS.to_owned(), "text/javascript; charset=utf-8"),
        _ => return AppError::not_found().into_response(),
    };
    let mut response = body.into_response();
    let headers = response.headers_mut();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert(
        CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=31536000, immutable"),
    );
    response
}

// ---------------------------------------------------------------------------
// Sign-in
// ---------------------------------------------------------------------------

/// Where to go after signing in: `next` when it is a page of the panel.
fn safe_next(next: Option<&str>, base: &str) -> String {
    match next {
        Some(next)
            if (next == base
                || next.starts_with(&format!("{base}/"))
                || next.starts_with(&format!("{base}?")))
                && !next.contains("//")
                && !next.contains('\\')
                && !next.starts_with(&format!("{base}/login"))
                && !next.starts_with(&format!("{base}/logout")) =>
        {
            next.to_owned()
        }
        _ => base.to_owned(),
    }
}

/// The parts of a sign-in request.
struct SignIn {
    app: AppContext,
    core: Arc<Core>,
    host: String,
    headers: HeaderMap,
    meta: ClientMeta,
    signed_in: bool,
}

impl SignIn {
    /// The parts of `request` and its body; `None` outside the panel.
    async fn from(request: Request) -> Option<(SignIn, Bytes)> {
        let (mut parts, body) = request.into_parts();
        let app = parts.extensions.get::<AppContext>().cloned()?;
        let core = app.get::<Core>().await.ok()?;
        let host = request_host(&http::Request::from_parts(parts.clone(), ()));
        let meta = ClientMeta::from_request_parts(&mut parts)
            .await
            .unwrap_or_else(|never| match never {});
        let signed_in = parts
            .extensions
            .get::<AuthState>()
            .and_then(|s| s.0.as_ref())
            .is_some_and(|p| p.via == AuthVia::Session);
        let bytes = to_bytes(body, 64 * 1024).await.unwrap_or_default();
        Some((
            SignIn {
                app,
                core,
                host,
                headers: parts.headers,
                meta,
                signed_in,
            },
            bytes,
        ))
    }

    /// The sign-in screen, with a fresh form token.
    fn screen(&self, status: StatusCode, extra: Value, available: bool) -> Response {
        let token = random_token(32);
        let mut ctx = json!({
            "panel": panel_json(&self.core, ""),
            "step": "password",
            "email": "",
            "next": self.core.options.base(),
            "error": Value::Null,
            "challenge": "",
            "unavailable": !available,
            "login_token": token,
        });
        if let (Value::Object(ctx), Value::Object(extra)) = (&mut ctx, extra) {
            ctx.extend(extra);
        }
        let mut response = render(&self.core, status, "helm/login.html", ctx);
        let path = format!("{}/login", self.core.options.base());
        if let Some(header) = build_cookie(LOGIN_COOKIE, token, &path, 3600, &self.host) {
            response.headers_mut().append(SET_COOKIE, header);
        }
        response
    }

    /// Whether the form's token matches the sign-in cookie.
    fn token_ok(&self, form: &HashMap<String, String>) -> bool {
        match (cookie(&self.headers, LOGIN_COOKIE), form.get("_login")) {
            (Some(expected), Some(given)) => {
                !expected.is_empty() && constant_time_eq(expected.as_bytes(), given.as_bytes())
            }
            _ => false,
        }
    }

    async fn services(&self) -> Option<(Arc<AuthService>, Arc<Encrypter>)> {
        let auth = self.app.try_get::<AuthService>().await?;
        let encrypter = self.app.try_get::<Encrypter>().await?;
        encrypter.encrypt_string("probe").ok()?;
        Some((auth, encrypter))
    }

    /// Signs `user` in: the session cookie, then `next`.
    fn start(
        &self,
        encrypter: &Encrypter,
        user: uuid::Uuid,
        refresh_token: String,
        next: Option<&str>,
    ) -> Response {
        let data = SessionData::new(user, Some(refresh_token), self.core.options.session_minutes);
        let mut response = redirect(&safe_next(next, self.core.options.base()));
        if let Some(header) = session_cookie(&self.core, encrypter, &data, &self.host) {
            response.headers_mut().append(SET_COOKIE, header);
        }
        let path = format!("{}/login", self.core.options.base());
        if let Some(header) = clear_cookie(LOGIN_COOKIE, &path, &self.host) {
            response.headers_mut().append(SET_COOKIE, header);
        }
        response
    }

    fn auth_error(&self, error: AuthError, step: Value) -> Response {
        let app_error = error.into_app_error();
        let message = match app_error.status.as_u16() {
            401 => "These credentials do not match our records.".to_owned(),
            429 => {
                let wait = app_error
                    .headers
                    .get(http::header::RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("60")
                    .to_owned();
                format!("Too many attempts. Try again in {wait} seconds.")
            }
            _ => return page_error(&self.core, app_error),
        };
        let mut extra = json!({ "error": message });
        if let (Value::Object(extra), Value::Object(step)) = (&mut extra, step) {
            extra.extend(step);
        }
        let mut response = self.screen(app_error.status, extra, true);
        if let Some(retry) = app_error.headers.get(http::header::RETRY_AFTER) {
            response
                .headers_mut()
                .insert(http::header::RETRY_AFTER, retry.clone());
        }
        response
    }
}

/// The fields of a urlencoded form (the last value of each name).
fn form_fields(bytes: &[u8]) -> HashMap<String, String> {
    form_urlencoded::parse(bytes).into_owned().collect()
}

/// `GET {path}/login`: the sign-in screen.
pub(crate) async fn login_form(RawQuery(query): RawQuery, request: Request) -> Response {
    let Some((signin, _)) = SignIn::from(request).await else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let next = form_urlencoded::parse(query.unwrap_or_default().as_bytes())
        .find(|(k, _)| k == "next")
        .map(|(_, v)| v.into_owned());
    let base = signin.core.options.base().to_owned();
    if signin.signed_in {
        return redirect(&safe_next(next.as_deref(), &base));
    }
    let available = signin.services().await.is_some();
    signin.screen(
        StatusCode::OK,
        json!({ "next": safe_next(next.as_deref(), &base) }),
        available,
    )
}

/// `POST {path}/login`: checks the password (with the login throttle of
/// `POST /api/auth/login`), then signs in or asks for the second factor.
pub(crate) async fn login(request: Request) -> Response {
    let Some((signin, body)) = SignIn::from(request).await else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let form = form_fields(&body);
    let base = signin.core.options.base().to_owned();
    let next = safe_next(form.get("next").map(String::as_str), &base);
    if !signin.token_ok(&form) {
        return signin.screen(
            StatusCode::FORBIDDEN,
            json!({ "error": "The sign-in form expired. Try again.", "next": next }),
            true,
        );
    }
    let Some((auth, encrypter)) = signin.services().await else {
        return signin.screen(StatusCode::SERVICE_UNAVAILABLE, json!({}), false);
    };
    let email = form.get("email").cloned().unwrap_or_default();
    let password = form.get("password").cloned().unwrap_or_default();
    match auth.attempt_login(&email, &password, &signin.meta).await {
        Ok(LoginOutcome::Authenticated { user, tokens }) => {
            signin.start(&encrypter, user.id, tokens.refresh_token, Some(&next))
        }
        Ok(LoginOutcome::TwoFactorRequired {
            challenge_token, ..
        }) => signin.screen(
            StatusCode::OK,
            json!({ "step": "two_factor", "challenge": challenge_token, "next": next }),
            true,
        ),
        Err(error) => signin.auth_error(error, json!({ "email": email, "next": next })),
    }
}

/// `POST {path}/login/two-factor`: the second factor of a sign-in.
pub(crate) async fn two_factor(request: Request) -> Response {
    let Some((signin, body)) = SignIn::from(request).await else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let form = form_fields(&body);
    let base = signin.core.options.base().to_owned();
    let next = safe_next(form.get("next").map(String::as_str), &base);
    let challenge = form.get("challenge_token").cloned().unwrap_or_default();
    let step = json!({ "step": "two_factor", "challenge": challenge, "next": next });
    if !signin.token_ok(&form) {
        return signin.screen(
            StatusCode::FORBIDDEN,
            json!({ "error": "The sign-in form expired. Sign in again.", "next": next }),
            true,
        );
    }
    let Some((auth, encrypter)) = signin.services().await else {
        return signin.screen(StatusCode::SERVICE_UNAVAILABLE, json!({}), false);
    };
    let code = form
        .get("code")
        .map(|c| c.replace(' ', ""))
        .unwrap_or_default();
    let recovery = form.get("recovery_code").cloned().unwrap_or_default();
    let proof = if !code.is_empty() {
        TwoFactorProof::Code(code)
    } else {
        TwoFactorProof::RecoveryCode(recovery.trim().to_owned())
    };
    match auth
        .verify_two_factor(&challenge, proof, &signin.meta)
        .await
    {
        Ok((user, tokens)) => signin.start(&encrypter, user.id, tokens.refresh_token, Some(&next)),
        Err(AuthError::InvalidTwoFactorCode) => {
            let mut extra = step;
            extra["error"] = json!("That code is not valid. Try again.");
            signin.screen(StatusCode::UNAUTHORIZED, extra, true)
        }
        Err(error) => signin.auth_error(error, step),
    }
}

/// `POST {path}/workspace`: works in the workspace named by the form's
/// `workspace` field from now on (a cookie), or in none when it is empty.
pub(crate) async fn choose_workspace(
    panel: Panel,
    headers: HeaderMap,
    request: Request,
) -> Response {
    let core = panel.cx.core.clone();
    let base = core.options.base().to_owned();
    let host = request_host(&request);
    let body = to_bytes(request.into_body(), 16 * 1024)
        .await
        .unwrap_or_default();
    let chosen = form_urlencoded::parse(&body)
        .find(|(name, _)| name == "workspace")
        .map(|(_, value)| value.trim().to_owned())
        .unwrap_or_default();
    // The workspace of a request that is inside one is not the panel's to
    // change.
    let fixed = panel.cx.uow.workspace().is_some() && requested(&headers).is_none();
    if fixed {
        return redirect_with(
            &base,
            "This page is inside a workspace already.",
            "warning",
            &core,
            &host,
        );
    }
    if chosen.is_empty() {
        let mut response = redirect(&base);
        if let Some(header) = clear_cookie(WORKSPACE_COOKIE, &base, &host) {
            response.headers_mut().append(SET_COOKIE, header);
        }
        return response;
    }
    let Ok(workspace) = uuid::Uuid::parse_str(&chosen) else {
        return redirect_with(&base, "That is not a workspace id.", "error", &core, &host);
    };
    let mut response = redirect(&base);
    if let Some(header) = build_cookie(
        WORKSPACE_COOKIE,
        workspace.to_string(),
        &base,
        60 * 60 * 24 * 30,
        &host,
    ) {
        response.headers_mut().append(SET_COOKIE, header);
    }
    response
}

/// `POST {path}/logout`: ends the panel session (and its refresh token).
pub(crate) async fn logout(request: Request) -> Response {
    let Some((signin, _)) = SignIn::from(request).await else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let base = signin.core.options.base().to_owned();
    if let Some(encrypter) = signin.app.try_get::<Encrypter>().await
        && let Some(value) = cookie(&signin.headers, SESSION_COOKIE)
        && let Ok(data) = encrypter.decrypt_json::<SessionData>(&value)
        && let Some(auth) = signin.app.try_get::<AuthService>().await
        && let Err(error) = auth.logout(data.r.as_deref(), data.u, false).await
    {
        tracing::warn!(target: "navree::helm", %error, "could not revoke the panel session's refresh token");
    }
    let mut response = redirect(&format!("{base}/login"));
    if let Some(header) = clear_cookie(SESSION_COOKIE, &base, &signin.host) {
        response.headers_mut().append(SET_COOKIE, header);
    }
    response
}

// ---------------------------------------------------------------------------
// Metrics
// ---------------------------------------------------------------------------

/// A metric card's template data.
fn metric_view(result: &MetricResult) -> Value {
    let mut view = serde_json::to_value(result).unwrap_or(Value::Null);
    let change_text = result
        .change
        .map(|c| format!("{:+.0}%", c * 100.0))
        .unwrap_or_default();
    view["change_text"] = json!(change_text);
    if !result.points.is_empty() {
        let max = result
            .points
            .iter()
            .map(|p| p.value)
            .fold(0.0_f64, f64::max)
            .max(1.0);
        let n = result.points.len();
        let polyline: Vec<String> = result
            .points
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let x = if n > 1 {
                    i as f64 / (n - 1) as f64 * 100.0
                } else {
                    50.0
                };
                let y = 31.0 - p.value / max * 29.0;
                format!("{x:.2},{y:.2}")
            })
            .collect();
        view["polyline"] = json!(polyline.join(" "));
        let first = &result.points[0];
        let last = &result.points[n - 1];
        view["summary"] = json!(format!(
            "from {} ({}) to {} ({})",
            first.label, first.display, last.label, last.display
        ));
    } else {
        view["polyline"] = json!("");
        view["summary"] = json!("");
    }
    let max = result
        .groups
        .iter()
        .map(|g| g.value)
        .fold(0.0_f64, f64::max);
    let groups: Vec<Value> = result
        .groups
        .iter()
        .map(|g| {
            let mut v = serde_json::to_value(g).unwrap_or(Value::Null);
            let percent = if max > 0.0 {
                g.value / max * 100.0
            } else {
                0.0
            };
            v["percent"] = json!(format!("{percent:.1}"));
            v
        })
        .collect();
    view["groups"] = json!(groups);
    view
}

/// The cards of `resource`'s metrics (only those on the dashboard when
/// `dashboard_only`); a failed metric is logged and left out.
async fn metric_views(
    cx: &Cx,
    resource: &dyn ErasedResource,
    range: crate::MetricRange,
    dashboard_only: bool,
) -> Vec<Value> {
    let mut cards = Vec::new();
    for metric in &resource.info().metrics {
        if dashboard_only && !metric.on_dashboard {
            continue;
        }
        match resource.metric(cx, &metric.slug, range).await {
            Ok(result) => cards.push(metric_view(&result)),
            Err(error) => tracing::warn!(
                target: "navree::helm",
                metric = %metric.slug,
                status = error.status.as_u16(),
                "a metric failed"
            ),
        }
    }
    cards
}

// ---------------------------------------------------------------------------
// Dashboard and lists
// ---------------------------------------------------------------------------

/// `GET {path}`: the dashboard.
pub(crate) async fn dashboard(panel: Panel, headers: HeaderMap) -> Response {
    let frame = Frame::new(&panel, &headers);
    let cx = &panel.cx;
    let core = &cx.core;
    let mut sections = Vec::new();
    if core.options.dashboard {
        for resource in core.registry.resources() {
            let info = resource.info();
            if !cx.can(READ, info.subject, None) || !info.metrics.iter().any(|m| m.on_dashboard) {
                continue;
            }
            let metrics = metric_views(cx, resource.as_ref(), Default::default(), true).await;
            if !metrics.is_empty() {
                sections.push(json!({
                    "label": info.label,
                    "url": format!("{}/resources/{}", core.options.base(), info.slug),
                    "metrics": metrics,
                }));
            }
        }
    }
    frame
        .page(
            StatusCode::OK,
            "helm/dashboard.html",
            "Dashboard",
            "dashboard",
            Vec::new(),
            json!({
                "resources": !core.registry.resources().is_empty(),
                "sections": sections,
            }),
        )
        .await
}

/// The column headers of a list.
fn columns(info: &ResourceInfo, q: &ListQuery, list_url: &str) -> Vec<Value> {
    let current = q.sort.clone().unwrap_or_else(|| info.default_sort.clone());
    screen_fields(info, Screen::Index)
        .into_iter()
        .map(|field| {
            let sortable = field.sortable && field.is_column();
            let sorted = if current == field.name {
                "asc"
            } else if current.strip_prefix('-') == Some(field.name.as_str()) {
                "desc"
            } else {
                ""
            };
            let next = if sorted == "asc" {
                format!("-{}", field.name)
            } else {
                field.name.clone()
            };
            json!({
                "name": field.name,
                "label": field.label_text(),
                "sortable": sortable,
                "sorted": sorted,
                "sort_url": if sortable {
                    Value::String(format!("{list_url}?{}", q.to_query(None, Some(&next))))
                } else {
                    Value::Null
                },
            })
        })
        .collect()
}

/// The rows of a table.
fn rows(items: &[Item]) -> Vec<Value> {
    items
        .iter()
        .map(|item| {
            json!({
                "id": item.id,
                "url": item.url,
                "title": item.title,
                "cells": item.cells,
                "can": item.can,
            })
        })
        .collect()
}

/// The actions shown on `screen`.
fn action_views(info: &ResourceInfo, detail: bool) -> Vec<Value> {
    info.actions
        .iter()
        .filter(|a| if detail { a.on_detail } else { a.on_index })
        .map(|a| {
            json!({
                "name": a.name,
                "label": a.label,
                "question": a.question,
                "destructive": a.destructive,
            })
        })
        .collect()
}

/// `(value, label, selected)` options of a relationship: the first records
/// of the related resource the user may read, plus the `selected` ones.
async fn relation_options(
    cx: &Cx,
    related: &dyn ErasedResource,
    selected: &[String],
) -> Result<Vec<Value>, AppError> {
    let mut found = related.lookup(cx, None, None, 100).await?;
    let missing: Vec<String> = selected
        .iter()
        .filter(|id| !found.iter().any(|(f, _)| f == *id))
        .cloned()
        .collect();
    if !missing.is_empty() {
        found.extend(
            related
                .lookup(cx, None, Some(&missing), missing.len() as u64)
                .await?,
        );
    }
    Ok(found
        .into_iter()
        .map(|(id, title)| json!({ "selected": selected.contains(&id), "value": id, "label": title }))
        .collect())
}

/// The filter panel.
async fn filter_views(cx: &Cx, info: &ResourceInfo, q: &ListQuery) -> Result<Vec<Value>, AppError> {
    let mut views = Vec::new();
    for filter in &info.filters {
        let values = q.filters.get(&filter.name).cloned().unwrap_or_default();
        let value = values.last().cloned().unwrap_or_default();
        let mut view = json!({
            "name": filter.name,
            "label": filter.label,
            "kind": filter.kind,
            "value": value,
            "options": [],
            "size": 1,
            "custom": "",
        });
        match filter.kind {
            "select" => {
                let options: Vec<Value> = filter
                    .options
                    .iter()
                    .map(|(v, l)| json!({ "value": v, "label": l, "selected": values.contains(v) }))
                    .collect();
                view["size"] = json!(options.len().clamp(2, 6));
                view["options"] = json!(options);
            }
            "belongs_to" => {
                if let Some(related) = filter
                    .related
                    .and_then(|r| cx.core.registry.by_type(r.type_id))
                {
                    let options = relation_options(cx, related.as_ref(), &values).await?;
                    view["size"] = json!(options.len().clamp(2, 6));
                    view["options"] = json!(options);
                }
            }
            "date_range" => {
                let presets = [
                    ("today", "Today"),
                    ("week", "This week"),
                    ("month", "This month"),
                    ("quarter", "This quarter"),
                ];
                let preset = values.iter().find(|v| presets.iter().any(|(p, _)| p == v));
                let custom = values.iter().find(|v| v.contains(".."));
                view["options"] = json!(presets
                    .iter()
                    .map(|(v, l)| json!({ "value": v, "label": l, "selected": preset.is_some_and(|p| p == v) }))
                    .collect::<Vec<_>>());
                view["custom"] = json!(custom.cloned().unwrap_or_default());
            }
            _ => {}
        }
        views.push(view);
    }
    Ok(views)
}

/// `GET {path}/resources/{resource}`: the list.
pub(crate) async fn list(
    panel: Panel,
    Path(slug): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let frame = Frame::new(&panel, &headers);
    let cx = &panel.cx;
    let result = async {
        let resource = resource(cx, &slug)?;
        let info = resource.info();
        let q = ListQuery::parse(query.as_deref().unwrap_or(""))?;
        let range = parse_range(query.as_deref())?;
        let page = resource.list(cx, &q).await?;
        let items = present(cx, resource.as_ref(), &page.records, Screen::Index).await?;
        let base = cx.core.options.base();
        let list_url = format!("{base}/resources/{}", info.slug);
        let link = |page: u64| format!("{list_url}?{}", q.to_query(Some(page), None));
        let meta = page.meta;
        let from = if items.is_empty() { 0 } else { (meta.current_page - 1) * meta.per_page + 1 };
        let mut range_links = Map::new();
        for r in crate::MetricRange::ALL {
            let mut ranged = q.clone();
            ranged.range = Some(r.as_str().to_owned());
            range_links.insert(
                r.as_str().to_owned(),
                json!(format!("{list_url}?{}", ranged.to_query(None, None))),
            );
        }
        let metrics = metric_views(cx, resource.as_ref(), range, false).await;
        let extra = json!({
            "resource": {
                "slug": info.slug,
                "label": info.label,
                "singular": info.singular,
                "url": list_url,
                "new_url": format!("{list_url}/new"),
                "actions_url": format!("{list_url}/actions"),
                "can_create": cx.can(CREATE, info.subject, None),
            },
            "columns": columns(info, &q, &list_url),
            "rows": rows(&items),
            "search": { "enabled": !info.search.is_empty(), "value": q.search.clone().unwrap_or_default() },
            "filters": filter_views(cx, info, &q).await?,
            "filtered": q.search.is_some() || !q.filters.is_empty() || q.via.is_some(),
            "sort": q.sort.clone().unwrap_or_default(),
            "via": q.via.as_ref().map(|(c, id)| json!({ "column": c, "id": id })),
            "actions": action_views(info, false),
            "metrics": metrics,
            "range_links": range_links,
            "return_url": format!("{list_url}?{}", q.to_query(Some(q.page), None)),
            "page": {
                "current": meta.current_page,
                "last": meta.last_page,
                "total": meta.total,
                "from": from,
                "to": from + items.len().saturating_sub(1) as u64,
                "prev_url": (meta.current_page > 1).then(|| link(meta.current_page - 1)),
                "next_url": (meta.current_page < meta.last_page).then(|| link(meta.current_page + 1)),
            },
        });
        Ok::<_, AppError>(
            frame
                .page(
                    StatusCode::OK,
                    "helm/list.html",
                    info.label,
                    &format!("resource:{}", info.slug),
                    vec![crumb(info.label, None)],
                    extra,
                )
                .await,
        )
    }
    .await;
    result.unwrap_or_else(|error| page_error(&cx.core, error))
}

// ---------------------------------------------------------------------------
// Records
// ---------------------------------------------------------------------------

/// `GET {path}/resources/{resource}/{id}`: a record's page.
pub(crate) async fn show(
    panel: Panel,
    Path((slug, id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let frame = Frame::new(&panel, &headers);
    let cx = &panel.cx;
    let result = async {
        let resource = resource(cx, &slug)?;
        let info = resource.info();
        let record = resource
            .find(cx, &id)
            .await?
            .ok_or_else(AppError::not_found)?;
        let item = present(
            cx,
            resource.as_ref(),
            std::slice::from_ref(&record),
            Screen::Detail,
        )
        .await?
        .remove(0);
        let relations = relation_panels(cx, info, &record).await?;
        let base = cx.core.options.base();
        let list_url = format!("{base}/resources/{}", info.slug);
        let extra = json!({
            "resource": {
                "slug": info.slug,
                "label": info.label,
                "singular": info.singular,
                "url": list_url,
                "actions_url": format!("{list_url}/actions"),
            },
            "item": item,
            "actions": action_views(info, true),
            "relations": relations,
        });
        Ok::<_, AppError>(
            frame
                .page(
                    StatusCode::OK,
                    "helm/detail.html",
                    &item.title,
                    &format!("resource:{}", info.slug),
                    vec![
                        crumb(info.label, Some(list_url.clone())),
                        crumb(item.title.clone(), None),
                    ],
                    extra,
                )
                .await,
        )
    }
    .await;
    result.unwrap_or_else(|error| page_error(&cx.core, error))
}

/// The `has_many` panels of a record's page.
async fn relation_panels(
    cx: &Cx,
    info: &ResourceInfo,
    record: &RawRecord,
) -> Result<Vec<Value>, AppError> {
    let base = cx.core.options.base();
    let mut panels = Vec::new();
    for field in info.fields.iter().filter(|f| f.on_detail) {
        let FieldKind::HasMany(related) = &field.kind else {
            continue;
        };
        let Some(target) = cx.core.registry.by_type(related.type_id) else {
            continue;
        };
        let target_info = target.info();
        if !cx.can(READ, target_info.subject, None) {
            continue;
        }
        let q = ListQuery {
            page: 1,
            per_page: Some(10),
            via: Some((field.name.clone(), record.id.clone())),
            ..ListQuery::default()
        };
        let page = target.list(cx, &q).await?;
        let items = present(cx, target.as_ref(), &page.records, Screen::Index).await?;
        let list_url = format!("{base}/resources/{}", target_info.slug);
        let mut columns = columns(target_info, &q, &list_url);
        let hidden: Vec<usize> = columns
            .iter()
            .enumerate()
            .filter(|(_, c)| c["name"] == field.name.as_str())
            .map(|(i, _)| i)
            .collect();
        let mut table_rows = rows(&items);
        for i in hidden.into_iter().rev() {
            columns.remove(i);
            for row in &mut table_rows {
                if let Some(cells) = row["cells"].as_array_mut() {
                    cells.remove(i);
                }
            }
        }
        panels.push(json!({
            "label": field.label.clone().unwrap_or_else(|| target_info.label.to_owned()),
            "total": page.meta.total,
            "url": format!("{list_url}?{}", q.to_query(None, None).replace("&per_page=10", "")),
            "actions_url": format!("{list_url}/actions"),
            "columns": columns,
            "rows": table_rows,
            "actions": action_views(target_info, false),
        }));
    }
    Ok(panels)
}

/// A form input's template data.
#[derive(Clone, Debug, Default, Serialize)]
struct Input {
    name: String,
    label: String,
    kind: &'static str,
    input_type: &'static str,
    help: Option<String>,
    required: bool,
    value: String,
    checked: bool,
    options: Vec<Value>,
    related: Option<String>,
    disabled: bool,
    locked: bool,
    errors: Vec<String>,
    min: Option<String>,
    max: Option<String>,
    step: Option<String>,
    minlength: Option<usize>,
    maxlength: Option<usize>,
    currency: Option<String>,
    accept: Option<String>,
}

/// The current text of a field in a form.
fn input_text(field: &Field, value: &Value, record: Option<&Value>) -> String {
    match (&field.kind, value) {
        (_, Value::Null) => String::new(),
        (FieldKind::Money { currency_column }, Value::Number(n)) => {
            let currency = record
                .and_then(|r| r.get(currency_column))
                .and_then(Value::as_str)
                .unwrap_or("USD");
            n.as_i64()
                .map(|cents| money::to_major(cents, currency))
                .unwrap_or_else(|| n.to_string())
        }
        (FieldKind::DateTime, Value::String(text)) => chrono::DateTime::parse_from_rfc3339(text)
            .map(|d| {
                d.with_timezone(&chrono::Utc)
                    .format("%Y-%m-%dT%H:%M")
                    .to_string()
            })
            .unwrap_or_else(|_| text.clone()),
        (_, value) => key_text(value).unwrap_or_else(|| value.to_string()),
    }
}

/// The inputs of a form: the record's values (or the defaults), replaced
/// by `submitted` text after a failed submit.
async fn inputs(
    cx: &Cx,
    fields: &[&Field],
    record: Option<&Value>,
    submitted: Option<&HashMap<String, String>>,
    errors: &BTreeMap<String, Vec<String>>,
    locked: &[String],
) -> Result<Vec<Input>, AppError> {
    let mut inputs = Vec::new();
    for field in fields {
        let stored = record
            .and_then(|r| r.get(&field.name))
            .cloned()
            .or_else(|| {
                if record.is_none() {
                    field.default.clone()
                } else {
                    None
                }
            })
            .unwrap_or(Value::Null);
        let text = match submitted.and_then(|s| s.get(&field.name)) {
            Some(text) => text.clone(),
            None => input_text(field, &stored, record),
        };
        let currency = match &field.kind {
            FieldKind::Money { currency_column } => Some(
                submitted
                    .and_then(|s| s.get(currency_column).cloned())
                    .or_else(|| {
                        record
                            .and_then(|r| r.get(currency_column))
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                    })
                    .unwrap_or_else(|| "USD".to_owned()),
            ),
            _ => None,
        };
        let (kind, input_type) = match field.kind {
            FieldKind::Number => ("number", "number"),
            FieldKind::Money { .. } => ("money", "number"),
            FieldKind::Email => ("email", "email"),
            FieldKind::Url => ("url", "url"),
            FieldKind::Date => ("date", "date"),
            FieldKind::DateTime => ("datetime", "datetime-local"),
            ref other => (other.name(), "text"),
        };
        let mut input = Input {
            name: field.name.clone(),
            label: field.label_text(),
            kind,
            input_type,
            help: field.help.clone(),
            required: field.required,
            checked: crate::values::boolean(&Value::String(text.clone())).unwrap_or(false),
            value: text.clone(),
            disabled: field.readonly || locked.contains(&field.name),
            locked: !field.readonly && locked.contains(&field.name),
            errors: errors.get(&field.name).cloned().unwrap_or_default(),
            minlength: field.min_length,
            maxlength: field.max_length,
            ..Input::default()
        };
        match &field.kind {
            FieldKind::Number => {
                input.step = Some("any".to_owned());
                input.min = field.min.map(|n| n.to_string());
                input.max = field.max.map(|n| n.to_string());
            }
            FieldKind::Money { .. } => {
                let decimals = money::minor_units(currency.as_deref().unwrap_or("USD"));
                input.step = Some(if decimals == 0 {
                    "1".to_owned()
                } else {
                    format!("0.{}1", "0".repeat(decimals as usize - 1))
                });
                input.currency = currency;
            }
            FieldKind::Select => {
                input.options = field
                    .options
                    .iter()
                    .map(|(v, l)| json!({ "value": v, "label": l, "selected": *v == text }))
                    .collect();
            }
            FieldKind::BelongsTo(related) => {
                if let Some(target) = cx.core.registry.by_type(related.type_id) {
                    let selected: Vec<String> = (!text.is_empty())
                        .then(|| text.clone())
                        .into_iter()
                        .collect();
                    input.options = relation_options(cx, target.as_ref(), &selected).await?;
                    input.related = Some(target.info().slug.clone());
                }
            }
            FieldKind::File { .. } => {
                input.accept = (!field.accept.is_empty()).then(|| field.accept.join(","));
            }
            _ => {}
        }
        inputs.push(input);
    }
    Ok(inputs)
}

/// What a form submitted: the values (last per name, `_`-prefixed names
/// dropped), every value of `ids`, and the internal fields.
#[derive(Debug, Default)]
struct Submission {
    values: HashMap<String, String>,
    ids: Vec<String>,
    meta: HashMap<String, String>,
    /// Files stored from a multipart form: field → path.
    files: HashMap<String, String>,
    /// Uploads refused (wrong type, too large, empty): field → messages.
    upload_errors: BTreeMap<String, Vec<String>>,
}

impl Submission {
    fn push(&mut self, name: String, value: String) {
        if name == "ids" || name == "ids[]" {
            self.ids.push(value);
        } else if let Some(meta) = name.strip_prefix('_') {
            self.meta.insert(meta.to_owned(), value);
        } else {
            self.values.insert(name, value);
        }
    }

    fn tz_offset(&self) -> Option<i32> {
        self.meta
            .get("tz_offset")
            .and_then(|v| v.trim().parse().ok())
    }
}

/// Reads a urlencoded or multipart form. With `uploads` (the create and
/// edit forms, after their ability checks), files sent for the resource's
/// file fields are stored on their disks; any other file is ignored.
async fn submission(
    cx: &Cx,
    info: &ResourceInfo,
    request: Request,
    uploads: bool,
) -> Result<Submission, AppError> {
    let multipart = request
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("multipart/form-data"));
    let mut submission = Submission::default();
    if !multipart {
        let bytes = to_bytes(request.into_body(), 16 * 1024 * 1024)
            .await
            .map_err(|_| AppError::bad_request("The form could not be read."))?;
        for (name, value) in form_urlencoded::parse(&bytes).into_owned() {
            submission.push(name, value);
        }
        return Ok(submission);
    }
    let mut form = Multipart::from_request(request)
        .await
        .map_err(|_| AppError::bad_request("The form could not be read."))?;
    while let Some(field) = form
        .next_field()
        .await
        .map_err(|_| AppError::bad_request("The form could not be read."))?
    {
        let name = field.name().unwrap_or_default().to_owned();
        let file_field = info
            .field(&name)
            .filter(|f| uploads && matches!(f.kind, FieldKind::File { .. }));
        if file_field.is_none() && field.file_name().is_some() {
            continue;
        }
        match file_field {
            Some(spec) => {
                if field.file_name().is_none_or(str::is_empty) {
                    continue;
                }
                let FieldKind::File { disk } = &spec.kind else {
                    continue;
                };
                let storage =
                    cx.app.get::<Storage>().await.map_err(|_| {
                        AppError::internal("file fields need StorageModule in the app")
                    })?;
                let disk = storage.disk(disk).map_err(AppError::internal)?;
                let rules = UploadRules::new(spec.max_size, spec.accept.clone());
                match store_upload(field, &disk, &info.slug, rules).await {
                    Ok(stored) => {
                        submission.files.insert(name, stored.path);
                    }
                    // A refused upload is shown under its field, with the
                    // rest of the form kept.
                    Err(error)
                        if matches!(
                            error.status,
                            StatusCode::BAD_REQUEST | StatusCode::PAYLOAD_TOO_LARGE
                        ) =>
                    {
                        let message = error
                            .errors
                            .as_deref()
                            .and_then(|errors| errors.get(&name))
                            .and_then(|messages| messages.first().cloned())
                            .or_else(|| error.detail.clone())
                            .unwrap_or_else(|| error.title.clone());
                        submission
                            .upload_errors
                            .entry(name)
                            .or_default()
                            .push(message);
                    }
                    Err(error) => return Err(error),
                }
            }
            None => {
                let value = field
                    .text()
                    .await
                    .map_err(|_| AppError::bad_request("The form could not be read."))?;
                submission.push(name, value);
            }
        }
    }
    Ok(submission)
}

/// The JSON input of a form submission for `fields` (money converted to
/// minor units, uploads to their paths).
fn form_input(
    fields: &[&Field],
    submission: &Submission,
    record: Option<&Value>,
) -> Map<String, Value> {
    let mut input = Map::new();
    for field in fields {
        if let Some(path) = submission.files.get(&field.name) {
            input.insert(field.name.clone(), Value::String(path.clone()));
            continue;
        }
        let Some(text) = submission.values.get(&field.name) else {
            continue;
        };
        if matches!(field.kind, FieldKind::File { .. }) {
            continue;
        }
        let value = match &field.kind {
            FieldKind::Money { currency_column } => {
                let currency = submission
                    .values
                    .get(currency_column)
                    .cloned()
                    .or_else(|| {
                        record
                            .and_then(|r| r.get(currency_column))
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                    })
                    .unwrap_or_else(|| "USD".to_owned());
                match money::parse_major(text, &currency) {
                    Some(cents) => Value::from(cents),
                    None => Value::String(text.clone()),
                }
            }
            _ => Value::String(text.clone()),
        };
        input.insert(field.name.clone(), value);
    }
    input
}

/// The fields of a form: editable fields; on edit also read-only ones
/// (shown disabled).
fn form_fields_of(info: &ResourceInfo, create: bool) -> Vec<&Field> {
    info.fields
        .iter()
        .filter(|f| f.in_forms() && (!create || f.editable()))
        .collect()
}

/// Renders the create or edit form.
#[allow(clippy::too_many_arguments)]
async fn form_page(
    frame: &Frame<'_>,
    resource: &dyn ErasedResource,
    status: StatusCode,
    record: Option<(&RawRecord, &Item)>,
    submitted: Option<&HashMap<String, String>>,
    error: Option<&AppError>,
) -> Result<Response, AppError> {
    let cx = frame.cx;
    let info = resource.info();
    let base = cx.core.options.base();
    let list_url = format!("{base}/resources/{}", info.slug);
    let create = record.is_none();
    let fields = form_fields_of(info, create);
    let errors: BTreeMap<String, Vec<String>> = error
        .and_then(|e| e.errors.as_deref().cloned())
        .unwrap_or_default();
    let general = error
        .filter(|e| e.errors.is_none())
        .map(|e| e.detail.clone().unwrap_or_else(|| e.title.clone()));
    let locked = record
        .map(|(_, item)| item.readonly.clone())
        .unwrap_or_default();
    let inputs = inputs(
        cx,
        &fields,
        record.map(|(r, _)| &r.json),
        submitted,
        &errors,
        &locked,
    )
    .await?;
    let multipart = fields
        .iter()
        .any(|f| matches!(f.kind, FieldKind::File { .. }));
    let (heading, action, submit, cancel, title, crumbs) = match record {
        None => (
            format!("New {}", info.singular.to_lowercase()),
            list_url.clone(),
            format!("Create {}", info.singular.to_lowercase()),
            list_url.clone(),
            format!("New {}", info.singular.to_lowercase()),
            vec![
                crumb(info.label, Some(list_url.clone())),
                crumb("New", None),
            ],
        ),
        Some((_, item)) => (
            format!("Edit {}", item.title),
            item.url.clone(),
            "Save changes".to_owned(),
            item.url.clone(),
            format!("Edit {}", item.title),
            vec![
                crumb(info.label, Some(list_url.clone())),
                crumb(item.title.clone(), Some(item.url.clone())),
                crumb("Edit", None),
            ],
        ),
    };
    let extra = json!({
        "resource": { "slug": info.slug, "label": info.label, "singular": info.singular },
        "heading": heading,
        "action": action,
        "submit": submit,
        "cancel_url": cancel,
        "fields": inputs,
        "multipart": multipart,
        "errors_summary": !errors.is_empty(),
        "general_error": general,
    });
    Ok(frame
        .page(
            status,
            "helm/form.html",
            &title,
            &format!("resource:{}", info.slug),
            crumbs,
            extra,
        )
        .await)
}

/// `GET {path}/resources/{resource}/new`: the create form.
pub(crate) async fn new_form(
    panel: Panel,
    Path(slug): Path<String>,
    headers: HeaderMap,
) -> Response {
    let frame = Frame::new(&panel, &headers);
    let cx = &panel.cx;
    let result = async {
        let resource = resource(cx, &slug)?;
        if !cx.can(CREATE, resource.info().subject, None) {
            return Err(AppError::forbidden());
        }
        form_page(&frame, resource.as_ref(), StatusCode::OK, None, None, None).await
    }
    .await;
    result.unwrap_or_else(|error| page_error(&cx.core, error))
}

/// `POST {path}/resources/{resource}`: creates a record from the form.
pub(crate) async fn create(
    panel: Panel,
    Path(slug): Path<String>,
    headers: HeaderMap,
    request: Request,
) -> Response {
    let core = panel.cx.core.clone();
    create_record(panel, slug, headers, request)
        .await
        .unwrap_or_else(|error| page_error(&core, error))
}

async fn create_record(
    mut panel: Panel,
    slug: String,
    headers: HeaderMap,
    request: Request,
) -> Result<Response, AppError> {
    let resource = resource(&panel.cx, &slug)?.clone();
    let info = resource.info();
    // Checked before reading the form, so no upload is stored for a user
    // who may not create (the record's own rules are checked on save).
    if !panel.cx.can(CREATE, info.subject, None) {
        return Err(AppError::forbidden());
    }
    let submission = submission(&panel.cx, info, request, true).await?;
    panel.cx.tz_offset = submission.tz_offset();
    let frame = Frame::new(&panel, &headers);
    if !submission.upload_errors.is_empty() {
        let error = AppError::validation(submission.upload_errors.clone());
        return form_page(
            &frame,
            resource.as_ref(),
            StatusCode::BAD_REQUEST,
            None,
            Some(&submission.values),
            Some(&error),
        )
        .await;
    }
    let fields = form_fields_of(info, true);
    let input = form_input(&fields, &submission, None);
    match resource.create(frame.cx, input).await {
        Ok(record) => {
            let url = format!(
                "{}/resources/{}/{}",
                frame.base(),
                info.slug,
                encode(&record.id)
            );
            Ok(redirect_with(
                &url,
                &format!("{} created.", info.singular),
                "success",
                frame.core(),
                &frame.host,
            ))
        }
        Err(error) if error.status == StatusCode::BAD_REQUEST => {
            form_page(
                &frame,
                resource.as_ref(),
                StatusCode::BAD_REQUEST,
                None,
                Some(&submission.values),
                Some(&error),
            )
            .await
        }
        Err(error) => Err(error),
    }
}

/// `GET {path}/resources/{resource}/{id}/edit`: the edit form.
pub(crate) async fn edit_form(
    panel: Panel,
    Path((slug, id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let frame = Frame::new(&panel, &headers);
    let cx = &panel.cx;
    let result = async {
        let resource = resource(cx, &slug)?;
        let record = resource
            .find(cx, &id)
            .await?
            .ok_or_else(AppError::not_found)?;
        let item = present(
            cx,
            resource.as_ref(),
            std::slice::from_ref(&record),
            Screen::Detail,
        )
        .await?
        .remove(0);
        if !item.can.update {
            return Err(AppError::forbidden());
        }
        form_page(
            &frame,
            resource.as_ref(),
            StatusCode::OK,
            Some((&record, &item)),
            None,
            None,
        )
        .await
    }
    .await;
    result.unwrap_or_else(|error| page_error(&cx.core, error))
}

/// `POST {path}/resources/{resource}/{id}`: saves the edit form.
pub(crate) async fn update(
    panel: Panel,
    Path((slug, id)): Path<(String, String)>,
    headers: HeaderMap,
    request: Request,
) -> Response {
    let core = panel.cx.core.clone();
    update_record(panel, slug, id, headers, request)
        .await
        .unwrap_or_else(|error| page_error(&core, error))
}

async fn update_record(
    mut panel: Panel,
    slug: String,
    id: String,
    headers: HeaderMap,
    request: Request,
) -> Result<Response, AppError> {
    let resource = resource(&panel.cx, &slug)?.clone();
    let info = resource.info();
    let record = resource
        .find(&panel.cx, &id)
        .await?
        .ok_or_else(AppError::not_found)?;
    let item = present(
        &panel.cx,
        resource.as_ref(),
        std::slice::from_ref(&record),
        Screen::Detail,
    )
    .await?
    .remove(0);
    // Checked before reading the form, so no upload is stored for a user
    // who may not change the record.
    if !item.can.update {
        return Err(AppError::forbidden());
    }
    let submission = submission(&panel.cx, info, request, true).await?;
    panel.cx.tz_offset = submission.tz_offset();
    let frame = Frame::new(&panel, &headers);
    if !submission.upload_errors.is_empty() {
        let error = AppError::validation(submission.upload_errors.clone());
        return form_page(
            &frame,
            resource.as_ref(),
            StatusCode::BAD_REQUEST,
            Some((&record, &item)),
            Some(&submission.values),
            Some(&error),
        )
        .await;
    }
    let fields: Vec<&Field> = form_fields_of(info, false)
        .into_iter()
        .filter(|f| f.editable() && !item.readonly.contains(&f.name))
        .collect();
    let input = form_input(&fields, &submission, Some(&record.json));
    match resource.update(frame.cx, &id, input).await {
        Ok(_) => Ok(redirect_with(
            &item.url,
            &format!("{} updated.", info.singular),
            "success",
            frame.core(),
            &frame.host,
        )),
        Err(error) if error.status == StatusCode::BAD_REQUEST => {
            form_page(
                &frame,
                resource.as_ref(),
                StatusCode::BAD_REQUEST,
                Some((&record, &item)),
                Some(&submission.values),
                Some(&error),
            )
            .await
        }
        Err(error) => Err(error),
    }
}

/// `POST {path}/resources/{resource}/{id}/delete`: deletes a record (after
/// a confirmation page when the browser did not ask already).
pub(crate) async fn destroy(
    panel: Panel,
    Path((slug, id)): Path<(String, String)>,
    headers: HeaderMap,
    request: Request,
) -> Response {
    let frame = Frame::new(&panel, &headers);
    let cx = &panel.cx;
    let result = async {
        let resource = resource(cx, &slug)?;
        let info = resource.info();
        let submission = submission(cx, info, request, false).await?;
        let base = cx.core.options.base();
        let list_url = format!("{base}/resources/{}", info.slug);
        if submission.meta.get("confirmed").map(String::as_str) != Some("1") {
            let record = resource.find(cx, &id).await?.ok_or_else(AppError::not_found)?;
            let action = ActionInfo {
                name: String::new(),
                label: format!("Delete {}", info.singular.to_lowercase()),
                ability: navree_access::DELETE,
                question: Some(format!("Delete {}?", record.title)),
                destructive: true,
                fields: Vec::new(),
                on_index: false,
                on_detail: true,
                queued: false,
            };
            let item_url = format!("{list_url}/{}", encode(&record.id));
            let extra = json!({
                "resource": { "slug": info.slug, "label": info.label, "actions_url": format!("{item_url}/delete") },
                "action": { "name": "", "label": action.label, "question": action.question, "destructive": true },
                "ids": [],
                "count": 1,
                "fields": [],
                "return_url": item_url,
                "general_error": Value::Null,
            });
            return Ok(frame
                .page(StatusCode::OK, "helm/action.html", &action.label, &format!("resource:{}", info.slug), Vec::new(), extra)
                .await);
        }
        resource.delete(cx, &id).await?;
        Ok(redirect_with(
            &list_url,
            &format!("{} deleted.", info.singular),
            "success",
            frame.core(),
            &frame.host,
        ))
    }
    .await;
    result.unwrap_or_else(|error| page_error(&cx.core, error))
}

/// Where to go back to after an action: `_return` when it is a page of
/// the panel.
fn return_url(submission: &Submission, base: &str, fallback: &str) -> String {
    match submission.meta.get("return") {
        Some(url)
            if (url.starts_with(&format!("{base}/")) || url == base)
                && !url.contains("//")
                && !url.contains('\\') =>
        {
            url.clone()
        }
        _ => fallback.to_owned(),
    }
}

/// `POST {path}/resources/{resource}/actions`: runs the action named by
/// the pressed button on the ticked rows; shows its form or a confirmation
/// first when it has one.
pub(crate) async fn run_action(
    panel: Panel,
    Path(slug): Path<String>,
    headers: HeaderMap,
    request: Request,
) -> Response {
    let core = panel.cx.core.clone();
    action_run(panel, slug, headers, request)
        .await
        .unwrap_or_else(|error| page_error(&core, error))
}

/// What the action page shows.
struct ActionPage<'a> {
    info: &'a ResourceInfo,
    action: &'a ActionInfo,
    submission: &'a Submission,
    back: &'a str,
}

/// The action's form (its fields and question) for the ticked rows.
async fn action_form(
    frame: &Frame<'_>,
    page: &ActionPage<'_>,
    status: StatusCode,
    error: Option<&AppError>,
) -> Result<Response, AppError> {
    let ActionPage {
        info,
        action,
        submission,
        back,
    } = page;
    let errors: BTreeMap<String, Vec<String>> = error
        .and_then(|e| e.errors.as_deref().cloned())
        .unwrap_or_default();
    let general = error
        .filter(|e| e.errors.is_none())
        .map(|e| e.detail.clone().unwrap_or_else(|| e.title.clone()));
    let submitted = error.is_some().then_some(&submission.values);
    let fields: Vec<&Field> = action.fields.iter().collect();
    let inputs = inputs(frame.cx, &fields, None, submitted, &errors, &[]).await?;
    let list_url = format!("{}/resources/{}", frame.base(), info.slug);
    let extra = json!({
        "resource": { "slug": info.slug, "label": info.label, "actions_url": format!("{list_url}/actions") },
        "action": { "name": action.name, "label": action.label, "question": action.question, "destructive": action.destructive },
        "ids": submission.ids,
        "count": submission.ids.len(),
        "fields": inputs,
        "return_url": back,
        "general_error": general,
    });
    Ok(frame
        .page(
            status,
            "helm/action.html",
            &action.label,
            &format!("resource:{}", info.slug),
            vec![
                crumb(info.label, Some(list_url)),
                crumb(action.label.clone(), None),
            ],
            extra,
        )
        .await)
}

async fn action_run(
    mut panel: Panel,
    slug: String,
    headers: HeaderMap,
    request: Request,
) -> Result<Response, AppError> {
    let resource = resource(&panel.cx, &slug)?.clone();
    let info = resource.info();
    let submission = submission(&panel.cx, info, request, false).await?;
    panel.cx.tz_offset = submission.tz_offset();
    let frame = Frame::new(&panel, &headers);
    let base = frame.base().to_owned();
    let list_url = format!("{base}/resources/{}", info.slug);
    let back = return_url(&submission, &base, &list_url);
    let name = submission.values.get("action").cloned().unwrap_or_default();
    let Some(action) = info.actions.iter().find(|a| a.name == name) else {
        return Err(AppError::not_found().with_detail(format!("No action `{name}`.")));
    };
    if submission.ids.is_empty() {
        return Ok(redirect_with(
            &back,
            "Select at least one row first.",
            "warning",
            frame.core(),
            &frame.host,
        ));
    }
    let page = ActionPage {
        info,
        action,
        submission: &submission,
        back: &back,
    };
    let step = submission.meta.get("step").map(String::as_str);
    let confirmed = submission.meta.get("confirmed").map(String::as_str) == Some("1");
    if (!action.fields.is_empty() && step != Some("run"))
        || (action.question.is_some() && !confirmed)
    {
        return action_form(&frame, &page, StatusCode::OK, None).await;
    }
    let fields: Vec<&Field> = action.fields.iter().collect();
    let input = form_input(&fields, &submission, None);
    let outcome = match resource
        .run_action(frame.cx, &name, submission.ids.clone(), input, false)
        .await
    {
        Ok(outcome) => outcome,
        Err(error) if error.status == StatusCode::BAD_REQUEST && !action.fields.is_empty() => {
            return action_form(&frame, &page, StatusCode::BAD_REQUEST, Some(&error)).await;
        }
        Err(error) if error.status == StatusCode::FORBIDDEN => {
            return Ok(redirect_with(
                &back,
                error
                    .detail
                    .as_deref()
                    .unwrap_or("You may not run this action."),
                "error",
                frame.core(),
                &frame.host,
            ));
        }
        Err(error) => return Err(error),
    };
    let skipped = if outcome.skipped > 0 {
        format!(
            " {} skipped: you may not run it on {}.",
            outcome.skipped,
            if outcome.skipped == 1 {
                "that record"
            } else {
                "those records"
            }
        )
    } else {
        String::new()
    };
    match outcome.result {
        ActionResult::Message { message } => Ok(redirect_with(
            &back,
            &format!("{message}{skipped}"),
            if outcome.skipped > 0 {
                "warning"
            } else {
                "success"
            },
            frame.core(),
            &frame.host,
        )),
        ActionResult::Redirect { url } | ActionResult::Download { url }
            if url.starts_with('/') && !url.starts_with("//") =>
        {
            Ok(redirect(&url))
        }
        ActionResult::Redirect { url } | ActionResult::Download { url } => {
            let extra = json!({
                "heading": action.label,
                "text": format!("{} is done.{skipped}", action.label),
                "link": url,
                "link_label": "Open",
                "back_url": back,
            });
            Ok(frame
                .page(
                    StatusCode::OK,
                    "helm/message.html",
                    &action.label,
                    &format!("resource:{}", info.slug),
                    vec![
                        crumb(info.label, Some(list_url)),
                        crumb(action.label.clone(), None),
                    ],
                    extra,
                )
                .await)
        }
    }
}

// ---------------------------------------------------------------------------
// Custom pages
// ---------------------------------------------------------------------------

/// `GET {path}/pages/{page}`: a custom page.
pub(crate) async fn custom_page(
    panel: Panel,
    Path(slug): Path<String>,
    headers: HeaderMap,
) -> Response {
    let frame = Frame::new(&panel, &headers);
    let cx = &panel.cx;
    let result = async {
        let entry = cx
            .core
            .registry
            .page(&slug)
            .ok_or_else(AppError::not_found)?;
        let page = &entry.page;
        if let Some((action, subject)) = &page.can
            && !cx.can(action, subject, None)
        {
            return Err(AppError::forbidden());
        }
        let data = match &page.data {
            Some(f) => f(crate::PageContext::new(cx.inner(entry.module, Map::new()))).await?,
            None => Value::Null,
        };
        let views = cx
            .core
            .app_views
            .as_ref()
            .ok_or_else(|| AppError::internal("custom pages need ViewModule"))?;
        let template = page.template.as_deref().unwrap_or_default();
        let content = views
            .render(
                template,
                json!({
                    "title": page.title,
                    "user": user_json(cx).await,
                    "csrf_token": frame.csrf(),
                    "path": cx.core.options.base(),
                    "data": data,
                }),
            )
            .map_err(AppError::internal)?;
        let extra = json!({ "heading": page.title, "content": content });
        Ok::<_, AppError>(
            frame
                .page(
                    StatusCode::OK,
                    "helm/page.html",
                    &page.title,
                    &format!("page:{}", page.slug),
                    vec![crumb(page.title.clone(), None)],
                    extra,
                )
                .await,
        )
    }
    .await;
    result.unwrap_or_else(|error| page_error(&cx.core, error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_stays_inside_the_panel() {
        assert_eq!(
            safe_next(Some("/admin/resources/deals?page=2"), "/admin"),
            "/admin/resources/deals?page=2"
        );
        assert_eq!(safe_next(Some("https://evil.example"), "/admin"), "/admin");
        assert_eq!(safe_next(Some("//evil.example"), "/admin"), "/admin");
        assert_eq!(safe_next(Some("/administrator"), "/admin"), "/admin");
        assert_eq!(safe_next(Some("/admin/login"), "/admin"), "/admin");
        assert_eq!(safe_next(None, "/admin"), "/admin");
    }

    #[test]
    fn submissions_split_values_ids_and_internal_fields() {
        let mut s = Submission::default();
        for (k, v) in [
            ("title", "A"),
            ("ids", "1"),
            ("ids", "2"),
            ("_csrf", "x"),
            ("_tz_offset", "-120"),
            ("title", "B"),
        ] {
            s.push(k.to_owned(), v.to_owned());
        }
        assert_eq!(s.values["title"], "B");
        assert_eq!(s.ids, ["1", "2"]);
        assert_eq!(s.tz_offset(), Some(-120));
    }
}
