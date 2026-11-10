//! `theme-demo`: the live theme builder that the docs at navree.com frame, served
//! under `/demo/ui/`. It is a trimmed app with the toolkit's themes pages and
//! nothing else:
//!
//! | Page | Shows |
//! |---|---|
//! | `/demo/ui/themes/builder` | the theme builder: every control works; Save checks a draft and keeps nothing |
//! | `/demo/ui/themes` | every built-in theme, light and dark, and the theme menu |
//! | `/demo/ui/themes/layouts/{layout}` | one page in each shell layout |
//! | `/demo/ui/health` | `204`, so the docs can tell the demo is up |
//!
//! The toolkit serves its own files and the theme menu's route under
//! `/_navree/ui/`, as in any app.
//!
//! It keeps nothing between visitors: a draft is its address, the theme menu's
//! choice is the visitor's own cookie, and a theme that is saved or imported is
//! checked, then sent back, and not stored. Around every request:
//!
//! - a write (`POST`) from another site is refused with `403`, as the toolkit
//!   refuses one to its own appearance route: the browser's `Sec-Fetch-Site`,
//!   else the `Origin` against the `Host`;
//! - each page keeps the toolkit's content security policy, except that the site
//!   the demo is part of may frame it: `frame-ancestors 'self'` and
//!   `X-Frame-Options: SAMEORIGIN`, in place of `'none'` and `DENY`.
//!
//! [`settings`] gives the server's defaults: a per-address rate limit, a small
//! body limit, and the client's address read from the reverse proxy in front.
#![forbid(unsafe_code)]

use std::sync::Arc;

use http::header::{
    CONTENT_SECURITY_POLICY, HOST, HeaderMap, HeaderValue, ORIGIN, X_FRAME_OPTIONS,
};
use http::{Method, StatusCode};
use navree::http::extract::Request;
use navree::http::middleware::{self, Next};
use navree::http::response::{IntoResponse, Response};
use navree::http::{GlobalLayer, LayerSlot};
use navree::prelude::*;
use navree::ui::Shell;

pub mod pages;

/// Where the demo is mounted on the site.
pub const BASE: &str = "/demo/ui";

/// The port the demo listens on, on the loopback address, unless `PORT` says otherwise.
pub const PORT: &str = "8095";

/// The demo's server settings, as `APP_` variables: the environment can still
/// override each one.
///
/// - `APP_HTTP__ADDR`: the loopback address and `port`; the reverse proxy is the
///   only way in.
/// - `APP_HTTP__PER_IP_RATE_LIMIT`: 300 requests a minute per visitor, about a
///   change a second with room for the page's files.
/// - `APP_HTTP__TRUSTED_PROXIES`: the visitor's address comes from the proxy on
///   this machine, so the limit counts visitors, not the proxy.
/// - `APP_HTTP__BODY_LIMIT_BYTES`: 64 KiB, more than a stored theme's JSON.
pub fn settings(port: &str) -> Vec<(String, String)> {
    vec![
        ("APP_HTTP__ADDR".to_owned(), format!("127.0.0.1:{port}")),
        ("APP_HTTP__PER_IP_RATE_LIMIT".to_owned(), "300".to_owned()),
        (
            "APP_HTTP__TRUSTED_PROXIES".to_owned(),
            "127.0.0.1,::1".to_owned(),
        ),
        ("APP_HTTP__BODY_LIMIT_BYTES".to_owned(), "65536".to_owned()),
    ]
}

/// The demo: its shell, its pages and the rules around every request.
pub struct DemoModule;

impl Module for DemoModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.factory(Factory::<Shell>::new(|_| async {
            Ok(Arc::new(pages::shell()))
        }))
        .exports::<Shell>();
        m.http_layer(GlobalLayer::new(
            LayerSlot::Context,
            "theme-demo.guard",
            |_, router| Ok(router.layer(middleware::from_fn(guard))),
        ));
        m.controller::<pages::DemoPages>();
    }
}

/// Refuses a write from another site, and lets the site frame the demo's pages.
async fn guard(request: Request, next: Next) -> Response {
    let writes = !matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );
    if writes && !same_site(request.headers()) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let mut response = next.run(request).await;
    framed_by_the_site(response.headers_mut());
    response
}

/// Whether a request comes from this site: the browser's `Sec-Fetch-Site`, else
/// the `Origin` against the `Host`. A client that sends neither (an old browser,
/// a script) is let through: the demo stores nothing it could change.
fn same_site(headers: &HeaderMap) -> bool {
    if let Some(site) = headers.get("sec-fetch-site") {
        return matches!(site.to_str(), Ok("same-origin" | "none"));
    }
    match (headers.get(ORIGIN), headers.get(HOST)) {
        (Some(origin), Some(host)) => origin
            .to_str()
            .ok()
            .and_then(|origin| origin.split_once("://"))
            .is_some_and(|(_, rest)| host.to_str().is_ok_and(|host| host == rest)),
        (Some(_), None) => false,
        (None, _) => true,
    }
}

/// The toolkit's policy, with the site the demo is part of allowed to frame it.
fn framed_by_the_site(headers: &mut HeaderMap) {
    let framed = headers
        .get(CONTENT_SECURITY_POLICY)
        .and_then(|policy| policy.to_str().ok())
        .map(|policy| policy.replace("frame-ancestors 'none'", "frame-ancestors 'self'"));
    if let Some(policy) = framed
        && let Ok(value) = HeaderValue::from_str(&policy)
    {
        headers.insert(CONTENT_SECURITY_POLICY, value);
    }
    headers.insert(X_FRAME_OPTIONS, HeaderValue::from_static("SAMEORIGIN"));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(*name, HeaderValue::from_static(value));
        }
        map
    }

    #[test]
    fn a_write_from_this_site_passes_and_one_from_another_does_not() {
        assert!(same_site(&headers(&[("sec-fetch-site", "same-origin")])));
        assert!(!same_site(&headers(&[("sec-fetch-site", "cross-site")])));
        assert!(!same_site(&headers(&[("sec-fetch-site", "same-site")])));
        assert!(same_site(&headers(&[
            ("origin", "https://navree.com"),
            ("host", "navree.com")
        ])));
        assert!(!same_site(&headers(&[
            ("origin", "https://elsewhere.example"),
            ("host", "navree.com")
        ])));
        assert!(same_site(&headers(&[])));
    }

    #[test]
    fn the_site_may_frame_a_page_and_nothing_else_changes() {
        let mut map = HeaderMap::new();
        map.insert(
            CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(navree::ui::PAGE_CSP),
        );
        framed_by_the_site(&mut map);
        let policy = map[CONTENT_SECURITY_POLICY].to_str().unwrap();
        assert_eq!(
            policy,
            navree::ui::PAGE_CSP.replace("frame-ancestors 'none'", "frame-ancestors 'self'")
        );
        assert!(policy.contains("script-src 'self'"), "{policy}");
        assert_eq!(map[X_FRAME_OPTIONS], "SAMEORIGIN");
    }
}
