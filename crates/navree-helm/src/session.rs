//! The panel's sign-in session (an encrypted cookie scoped to the panel's
//! path), its CSRF check, and the request layer that also turns errors
//! into pages for browsers and sets the pages' security headers.

use axum::body::Body;
use axum::extract::Request;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use cookie::{Cookie, SameSite};
use http::header::{
    ACCEPT, AUTHORIZATION, CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, COOKIE, HOST,
    LOCATION, SET_COOKIE,
};
use http::{HeaderMap, HeaderValue, Method, StatusCode};
use navree_contracts::{AuthState, AuthVia, Principal};
use navree_crypto::{Encrypter, constant_time_eq, random_token};
use navree_http::{AppError, GlobalLayer, LayerSlot};
use navree_kernel::AppContext;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::core::Core;
use crate::ui;

/// The session cookie.
pub const SESSION_COOKIE: &str = "navree_helm";

/// The sign-in form's CSRF cookie (double submit, before there is a
/// session).
pub(crate) const LOGIN_COOKIE: &str = "navree_helm_login";

/// The one-time message shown after a redirect.
pub(crate) const FLASH_COOKIE: &str = "navree_helm_flash";

/// The sidebar's state on wide screens (`collapsed` or `expanded`), set by
/// the script when Ctrl/Cmd+B or the trigger toggles it, so the next page
/// renders the same way.
pub(crate) const SIDEBAR_COOKIE: &str = "navree_helm_sidebar";

/// The Content Security Policy of every panel response: scripts, styles
/// and connections from the app's origin only, nothing inline, no framing.
pub(crate) const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data: https:; connect-src 'self'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'";

/// What the session cookie holds (encrypted with the app key).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct SessionData {
    /// The user.
    pub(crate) u: Uuid,
    /// The CSRF token.
    pub(crate) c: String,
    /// Issued at (Unix seconds).
    pub(crate) i: i64,
    /// Expires at (Unix seconds).
    pub(crate) e: i64,
    /// The refresh token of the sign-in (revoked at sign-out).
    pub(crate) r: Option<String>,
}

impl SessionData {
    /// A new session for `user`, lasting `minutes`.
    pub(crate) fn new(user: Uuid, refresh_token: Option<String>, minutes: u64) -> Self {
        let now = Utc::now().timestamp();
        Self {
            u: user,
            c: random_token(40),
            i: now,
            e: now.saturating_add(i64::try_from(minutes.saturating_mul(60)).unwrap_or(i64::MAX)),
            r: refresh_token,
        }
    }
}

/// The panel session of the request, if it signed in with the cookie.
#[derive(Clone, Debug, Default)]
pub(crate) struct PanelSession {
    /// The CSRF token forms send back (`None` without a cookie session:
    /// bearer tokens need none).
    pub(crate) csrf: Option<String>,
}

/// The request layer of the panel (in the `AuthContext` slot, after the
/// app's authenticator).
pub(crate) fn layer() -> GlobalLayer {
    GlobalLayer::new(LayerSlot::AuthContext, "helm", |_ctx, router| {
        Ok(router.layer(axum::middleware::from_fn(panel_layer)))
    })
}

async fn panel_layer(mut request: Request, next: Next) -> Response {
    let Some(app) = request.extensions().get::<AppContext>().cloned() else {
        return next.run(request).await;
    };
    let Some(core) = app.try_get::<Core>().await else {
        return next.run(request).await;
    };
    let base = core.options.base().to_owned();
    let path = request.uri().path().to_owned();
    if path != base && !path.starts_with(&format!("{base}/")) {
        return next.run(request).await;
    }
    let assets = path.starts_with(&format!("{base}/assets/"));
    let html = !path.starts_with(&format!("{base}/api/")) && accepts_html(request.headers());
    let method = request.method().clone();
    let target = request
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str().to_owned())
        .unwrap_or_else(|| path.clone());

    let mut session = None;
    let guest = request
        .extensions()
        .get::<AuthState>()
        .is_none_or(|state| state.0.is_none());
    if guest
        && !request.headers().contains_key(AUTHORIZATION)
        && let Some(encrypter) = app.try_get::<Encrypter>().await
        && let Some(data) = read_session(request.headers(), &encrypter)
    {
        request.extensions_mut().insert(AuthState(Some(Principal {
            user_id: data.u,
            via: AuthVia::Session,
            token_abilities: None,
            session_id: Some(data.c.clone()),
        })));
        session = Some(data);
    }

    // The sign-in forms carry their own token (a double submit), as there
    // may be no session yet.
    let sign_in = path == format!("{base}/login") || path == format!("{base}/login/two-factor");
    if let Some(data) = &session
        && !is_safe(&method)
        && !sign_in
    {
        let (checked, ok) = check_csrf(request, &data.c).await;
        request = checked;
        if !ok {
            let error = AppError::csrf_mismatch();
            let response = if html {
                ui::status_page(&core, error.status, Some("Your session expired or the form is stale. Go back, reload the page and try again.".to_owned()))
            } else {
                error.into_response()
            };
            return finish(response, &core, assets, None, &request_host(&request));
        }
    }
    request.extensions_mut().insert(PanelSession {
        csrf: session.as_ref().map(|s| s.c.clone()),
    });
    let host = request_host(&request);
    let response = next.run(request).await;
    let response = if html {
        browser_errors(response, &core, &method, &target).await
    } else {
        response
    };
    let renew = session.filter(|data| {
        let half = (data.e - data.i) / 2;
        method == Method::GET && html && Utc::now().timestamp() - data.i > half
    });
    let renewal = match renew {
        Some(data) => match app.try_get::<Encrypter>().await {
            Some(encrypter) => {
                let renewed = SessionData {
                    i: Utc::now().timestamp(),
                    e: Utc::now().timestamp()
                        + i64::try_from(core.options.session_minutes.saturating_mul(60))
                            .unwrap_or(i64::MAX),
                    ..data
                };
                session_cookie(&core, &encrypter, &renewed, &host)
            }
            None => None,
        },
        None => None,
    };
    finish(response, &core, assets, renewal, &host)
}

/// Sets the panel's headers: CSP, no caching for anything but assets, and
/// a renewed session cookie.
fn finish(
    mut response: Response,
    _core: &Core,
    assets: bool,
    renewal: Option<HeaderValue>,
    _host: &str,
) -> Response {
    let headers = response.headers_mut();
    headers.insert(CONTENT_SECURITY_POLICY, HeaderValue::from_static(CSP));
    if !assets {
        headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    if let Some(cookie) = renewal {
        headers.append(SET_COOKIE, cookie);
    }
    response
}

/// For browsers: client errors in the JSON shape (from the framework's
/// checks: a 401 before the sign-in, a 403 without `view` on `Helm`, an
/// unknown path) become a redirect to the sign-in page (401) or an error
/// page. The panel's own pages render their errors themselves.
async fn browser_errors(
    response: Response,
    core: &Core,
    method: &Method,
    target: &str,
) -> Response {
    let status = response.status();
    let json = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/json"));
    if !json || !status.is_client_error() {
        return response;
    }
    if status == StatusCode::UNAUTHORIZED {
        let base = core.options.base();
        let next = if *method == Method::GET { target } else { base };
        let location = format!(
            "{base}/login?next={}",
            form_urlencoded::byte_serialize(next.as_bytes()).collect::<String>()
        );
        return redirect(&location);
    }
    let (parts, body) = response.into_parts();
    let bytes = axum::body::to_bytes(body, 64 * 1024)
        .await
        .unwrap_or_default();
    let message = serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()
        .and_then(|body| match &body["message"] {
            serde_json::Value::String(text) => Some(text.clone()),
            serde_json::Value::Array(items) => Some(
                items
                    .iter()
                    .filter_map(|m| m.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            _ => None,
        });
    let mut page = ui::status_page(core, parts.status, message);
    for name in [http::header::RETRY_AFTER, http::header::WWW_AUTHENTICATE] {
        if let Some(value) = parts.headers.get(&name) {
            page.headers_mut().insert(name, value.clone());
        }
    }
    page
}

/// A `303 See Other` to `location`.
pub(crate) fn redirect(location: &str) -> Response {
    let mut response = StatusCode::SEE_OTHER.into_response();
    if let Ok(value) = HeaderValue::from_str(location) {
        response.headers_mut().insert(LOCATION, value);
    }
    response
}

/// Whether the client asked for HTML (a browser navigation).
fn accepts_html(headers: &HeaderMap) -> bool {
    headers
        .get(ACCEPT)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("text/html"))
}

fn is_safe(method: &Method) -> bool {
    matches!(
        *method,
        Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE
    )
}

/// The value of cookie `name` in the request.
pub(crate) fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| Cookie::parse_encoded(pair.trim().to_owned()).ok())
        .find(|c| c.name() == name)
        .map(|c| c.value().to_owned())
}

/// The valid, unexpired session in the request's cookie.
fn read_session(headers: &HeaderMap, encrypter: &Encrypter) -> Option<SessionData> {
    let value = cookie(headers, SESSION_COOKIE)?;
    let data: SessionData = encrypter.decrypt_json(&value).ok()?;
    (data.e > Utc::now().timestamp()).then_some(data)
}

/// The request's host, without the port.
pub(crate) fn request_host<B>(request: &http::Request<B>) -> String {
    request
        .headers()
        .get(HOST)
        .and_then(|v| v.to_str().ok())
        .or_else(|| request.uri().host())
        .map(|host| {
            if let Some(rest) = host.strip_prefix('[') {
                rest.split(']').next().unwrap_or(rest).to_owned()
            } else {
                host.rsplit_once(':').map_or(host, |(h, _)| h).to_owned()
            }
        })
        .unwrap_or_default()
}

/// Whether cookies for `host` may skip the `Secure` flag (local
/// development over plain HTTP).
fn is_loopback(host: &str) -> bool {
    host.is_empty()
        || host == "localhost"
        || host.ends_with(".localhost")
        || host == "::1"
        || host.starts_with("127.")
}

/// A cookie for the panel: httpOnly, `SameSite=Strict`, scoped to `path`,
/// `Secure` except on loopback hosts.
pub(crate) fn build_cookie(
    name: &str,
    value: String,
    path: &str,
    max_age_secs: i64,
    host: &str,
) -> Option<HeaderValue> {
    let cookie = Cookie::build((name.to_owned(), value))
        .path(path.to_owned())
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(!is_loopback(host))
        .max_age(cookie::time::Duration::seconds(max_age_secs))
        .build();
    HeaderValue::from_str(&cookie.encoded().to_string()).ok()
}

/// The `Set-Cookie` header of `data`.
pub(crate) fn session_cookie(
    core: &Core,
    encrypter: &Encrypter,
    data: &SessionData,
    host: &str,
) -> Option<HeaderValue> {
    let value = encrypter.encrypt_json(data).ok()?;
    build_cookie(
        SESSION_COOKIE,
        value,
        core.options.base(),
        data.e - Utc::now().timestamp(),
        host,
    )
}

/// The `Set-Cookie` header that removes cookie `name` at `path`.
pub(crate) fn clear_cookie(name: &str, path: &str, host: &str) -> Option<HeaderValue> {
    build_cookie(name, String::new(), path, 0, host)
}

/// Checks the CSRF token of a state-changing request: the `X-CSRF-Token`
/// header, a `_csrf` query parameter (multipart forms) or a `_csrf` form
/// field. Returns the request (its body put back) and whether it matched.
async fn check_csrf(request: Request, expected: &str) -> (Request, bool) {
    let matches = |token: &str| constant_time_eq(token.as_bytes(), expected.as_bytes());
    if let Some(token) = request
        .headers()
        .get("x-csrf-token")
        .and_then(|v| v.to_str().ok())
    {
        let ok = matches(token.trim());
        return (request, ok);
    }
    if let Some(query) = request.uri().query()
        && let Some((_, token)) =
            form_urlencoded::parse(query.as_bytes()).find(|(k, _)| k == "_csrf")
    {
        let ok = matches(&token);
        return (request, ok);
    }
    let form = request
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/x-www-form-urlencoded"));
    if !form {
        return (request, false);
    }
    let (parts, body) = request.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, 16 * 1024 * 1024).await else {
        return (Request::from_parts(parts, Body::empty()), false);
    };
    let ok = form_urlencoded::parse(&bytes)
        .find(|(k, _)| k == "_csrf")
        .is_some_and(|(_, token)| matches(&token));
    (Request::from_parts(parts, Body::from(bytes)), ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookies_are_strict_and_secure_off_loopback() {
        let header =
            build_cookie("navree_helm", "v".into(), "/admin", 60, "admin.example.com").unwrap();
        let text = header.to_str().unwrap();
        for part in [
            "navree_helm=v",
            "Path=/admin",
            "HttpOnly",
            "SameSite=Strict",
            "Secure",
            "Max-Age=60",
        ] {
            assert!(text.contains(part), "{text}");
        }
        let local = build_cookie("navree_helm", "v".into(), "/admin", 60, "127.0.0.1").unwrap();
        assert!(!local.to_str().unwrap().contains("Secure"));
    }

    #[test]
    fn hosts_drop_their_port() {
        let request = http::Request::builder()
            .header(HOST, "localhost:8080")
            .body(())
            .unwrap();
        assert_eq!(request_host(&request), "localhost");
        let request = http::Request::builder()
            .header(HOST, "[::1]:8080")
            .body(())
            .unwrap();
        assert_eq!(request_host(&request), "::1");
        assert!(is_loopback("localhost") && is_loopback("127.0.0.1") && !is_loopback("acme.com"));
    }

    #[test]
    fn cookies_are_read_from_the_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            HeaderValue::from_static("a=1; navree_helm=abc%3D; b=2"),
        );
        assert_eq!(cookie(&headers, SESSION_COOKIE).as_deref(), Some("abc="));
        assert_eq!(cookie(&headers, "missing"), None);
    }
}
