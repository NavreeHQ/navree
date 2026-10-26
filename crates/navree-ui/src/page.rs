//! Rendering a page: [`render`], the [`HtmlPage`] response with its content
//! security policy, and the sidebar cookie.

use axum::http::header::{CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, COOKIE};
use axum::http::{HeaderMap, HeaderValue};
use axum::response::{IntoResponse, Response};
use navree_view::{ViewError, Views};
use serde::Serialize;

use crate::Shell;

/// The content security policy of kit pages: scripts, styles, fonts and
/// API calls from the page's own origin only, images also as `data:` URLs,
/// no inline scripts or styles, no framing. The kit's markup and script need
/// nothing more.
pub const PAGE_CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; font-src 'self'; img-src 'self' data:; connect-src 'self'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'";

/// The cookie the script sets when the sidebar is collapsed or expanded
/// (`collapsed` or `expanded`, path `/`, one year).
pub const SIDEBAR_COOKIE: &str = "navree_ui_sidebar";

/// A rendered HTML page: `200`, `text/html; charset=utf-8`, [`PAGE_CSP`]
/// and `Cache-Control: no-store` (tool pages show private data).
#[derive(Clone, Debug)]
pub struct HtmlPage(pub String);

impl IntoResponse for HtmlPage {
    fn into_response(self) -> Response {
        (
            [
                (
                    CONTENT_TYPE,
                    HeaderValue::from_static("text/html; charset=utf-8"),
                ),
                (CONTENT_SECURITY_POLICY, HeaderValue::from_static(PAGE_CSP)),
                (CACHE_CONTROL, HeaderValue::from_static("no-store")),
            ],
            self.0,
        )
            .into_response()
    }
}

/// Whether the request's cookies say the sidebar is collapsed (pass it to
/// [`Shell::collapsed`], so the page renders the user's choice without a
/// flash).
pub fn sidebar_collapsed(headers: &HeaderMap) -> bool {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|cookies| cookies.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .any(|(name, value)| name == SIDEBAR_COOKIE && value == "collapsed")
}

/// Renders template `name` with `shell` (as the variable `shell`) and the
/// fields of `ctx` (a struct, a map or `serde_json::json!({..})`; `()` for
/// none), into an [`HtmlPage`].
///
/// ```
/// use navree_ui::{Shell, render};
/// use serde_json::json;
///
/// # fn main() -> Result<(), navree_view::ViewError> {
/// let views = navree_ui::views_with([(
///     "helm/hello.html",
///     r#"{% extends "ui/app.html" %}{% block content %}<p>Hello, {{ name }}</p>{% endblock %}"#,
/// )])?;
/// let page = render(&views, "helm/hello", &Shell::new("Helm"), json!({ "name": "Ada" }))?;
/// assert!(page.0.contains("<p>Hello, Ada</p>"));
/// assert!(page.0.contains(r#"class="ui-sidebar""#));
/// # Ok(())
/// # }
/// ```
pub fn render(
    views: &Views,
    name: &str,
    shell: &Shell,
    ctx: impl Serialize,
) -> Result<HtmlPage, ViewError> {
    let mut context = match serde_json::to_value(ctx) {
        Ok(serde_json::Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    };
    context.insert(
        "shell".to_owned(),
        serde_json::to_value(shell).unwrap_or(serde_json::Value::Null),
    );
    views.render(name, context).map(HtmlPage)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidebar_cookie_is_read_among_others() {
        let mut headers = HeaderMap::new();
        assert!(!sidebar_collapsed(&headers));
        headers.insert(
            COOKIE,
            HeaderValue::from_static("a=1; navree_ui_sidebar=collapsed; b=2"),
        );
        assert!(sidebar_collapsed(&headers));
        headers.insert(COOKIE, HeaderValue::from_static("navree_ui_sidebar=expanded"));
        assert!(!sidebar_collapsed(&headers));
    }

    #[test]
    fn html_pages_carry_the_policy() {
        let response = HtmlPage("<p>hi</p>".to_owned()).into_response();
        assert_eq!(
            response.headers()[CONTENT_SECURITY_POLICY],
            HeaderValue::from_static(PAGE_CSP)
        );
        assert_eq!(response.headers()[CACHE_CONTROL], "no-store");
        assert!(
            response.headers()[CONTENT_TYPE]
                .to_str()
                .unwrap()
                .starts_with("text/html")
        );
    }
}
