//! The panel's request layer: it hands each page the CSRF token of the
//! browser's session, turns errors into pages for browsers (a guest goes
//! to the app's sign-in page) and sets the pages' security headers.
//!
//! Who is signed in is not decided here. A browser signs in on the app's
//! session (navree-session, navree-auth's `SessionStrategy` and
//! `AuthPages`), the same session every tool and page of the app uses; an API
//! client sends a bearer token. Both reach the panel as the request's
//! `AuthState`.

use cookie::{Cookie, SameSite};
use http::header::{
    ACCEPT, CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, COOKIE, HOST, LOCATION,
};
use http::{HeaderMap, HeaderValue, Method, StatusCode};
use navree_contracts::{AuthState, AuthVia};
use navree_http::body::to_bytes;
use navree_http::extract::Request;
use navree_http::middleware::Next;
use navree_http::response::{IntoResponse, Response};
use navree_http::{GlobalLayer, LayerSlot};
use navree_kernel::AppContext;
use navree_session::Session;
use navree_ui::{PagesOptions, ReturnTo};

use crate::core::Core;
use crate::ui;

/// The one-time message shown after a redirect.
pub(crate) const FLASH_COOKIE: &str = "navree_helm_flash";

/// The sidebar's state on wide screens (`collapsed` or `expanded`), set by
/// the script when Ctrl/Cmd+B or the trigger toggles it, so the next page
/// renders the same way.
pub(crate) const SIDEBAR_COOKIE: &str = "navree_helm_sidebar";

/// Where the sign-in page is when the app has not said (`ui.sign_in`).
const DEFAULT_SIGN_IN: &str = "/sign-in";

/// The Content Security Policy of every panel response: scripts, styles
/// and connections from the app's origin only, nothing inline, no framing.
pub(crate) const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data: https:; connect-src 'self'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'";

/// The panel session of the request, if it signed in on the browser
/// session.
#[derive(Clone, Debug, Default)]
pub(crate) struct PanelSession {
    /// The CSRF token forms send back (`None` without a browser session:
    /// bearer tokens need none).
    pub(crate) csrf: Option<String>,
}

/// The request layer of the panel (in the `AuthContext` slot, after the
/// app's authenticator).
pub(crate) fn layer() -> GlobalLayer {
    GlobalLayer::new(LayerSlot::AuthContext, "helm", |_ctx, router| {
        Ok(router.layer(navree_http::middleware::from_fn(panel_layer)))
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

    // A browser signed in on the session gets the session's token for its
    // forms; the session layer checks it on every change.
    let browser = request
        .extensions()
        .get::<AuthState>()
        .and_then(|state| state.0.as_ref())
        .is_some_and(|principal| principal.via == AuthVia::Session);
    let csrf = if browser {
        request
            .extensions()
            .get::<Session>()
            .map(Session::csrf_token)
    } else {
        None
    };
    request.extensions_mut().insert(PanelSession { csrf });

    let response = next.run(request).await;
    let response = if html {
        let sign_in = sign_in_path(&app).await;
        browser_errors(response, &core, &method, &target, &sign_in).await
    } else {
        response
    };
    finish(response, assets)
}

/// Sets the panel's headers: the Content Security Policy, and no caching
/// for anything but assets.
fn finish(mut response: Response, assets: bool) -> Response {
    let headers = response.headers_mut();
    headers.insert(CONTENT_SECURITY_POLICY, HeaderValue::from_static(CSP));
    if !assets {
        headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    response
}

/// Where the app's sign-in page is: `ui.sign_in`, else `/sign-in`.
async fn sign_in_path(app: &AppContext) -> String {
    match app.try_get::<PagesOptions>().await {
        Some(options) => options.sign_in.clone(),
        None => DEFAULT_SIGN_IN.to_owned(),
    }
}

/// The sign-in page, asking to come back to `next` (the panel's home when
/// `next` is not a page of this site).
pub(crate) fn sign_in_location(sign_in: &str, next: &str, home: &str) -> String {
    ReturnTo::parse(next)
        .or_else(|| ReturnTo::parse(home))
        .map_or_else(
            || sign_in.to_owned(),
            |back| back.sign_in_href(sign_in).as_str().to_owned(),
        )
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
    sign_in: &str,
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
        return redirect(&sign_in_location(sign_in, next, base));
    }
    let (parts, body) = response.into_parts();
    let bytes = to_bytes(body, 64 * 1024).await.unwrap_or_default();
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

/// The `Set-Cookie` header that removes cookie `name` at `path`.
pub(crate) fn clear_cookie(name: &str, path: &str, host: &str) -> Option<HeaderValue> {
    build_cookie(name, String::new(), path, 0, host)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookies_are_strict_and_secure_off_loopback() {
        let header = build_cookie(
            FLASH_COOKIE,
            "v".into(),
            "/admin",
            60,
            "admin.example.com",
        )
        .unwrap();
        let text = header.to_str().unwrap();
        for part in [
            "navree_helm_flash=v",
            "Path=/admin",
            "HttpOnly",
            "SameSite=Strict",
            "Secure",
            "Max-Age=60",
        ] {
            assert!(text.contains(part), "{text}");
        }
        let local = build_cookie(FLASH_COOKIE, "v".into(), "/admin", 60, "127.0.0.1").unwrap();
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
            HeaderValue::from_static("a=1; navree_helm_flash=abc%3D; b=2"),
        );
        assert_eq!(cookie(&headers, FLASH_COOKIE).as_deref(), Some("abc="));
        assert_eq!(cookie(&headers, "missing"), None);
    }

    #[test]
    fn the_sign_in_page_gets_the_page_to_come_back_to() {
        assert_eq!(
            sign_in_location("/sign-in", "/admin/resources/deals?page=2", "/admin"),
            "/sign-in?next=%2Fadmin%2Fresources%2Fdeals%3Fpage%3D2"
        );
        // An address that leads off the site comes back to the panel's home.
        assert_eq!(
            sign_in_location("/login", "//evil.example/x", "/admin"),
            "/login?next=%2Fadmin"
        );
    }
}
