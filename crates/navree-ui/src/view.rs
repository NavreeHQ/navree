//! [`View`]: a whole page, with its content security policy.

use std::borrow::Cow;

use http::header::{CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, HeaderName, HeaderValue, SET_COOKIE};
use http::StatusCode;
use navree_http::response::{IntoResponse, Response};

use crate::assets;
use crate::html::escape_into;
use crate::node::{IntoNodes, Node, Uses};
use crate::render::{measure, render_into};

/// The content security policy of every page: scripts, styles, fonts and
/// requests from the page's own origin only, images also as `data:` URLs, no
/// inline scripts or styles, no framing.
pub const PAGE_CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; font-src 'self'; img-src 'self' data:; connect-src 'self'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'";

/// The `Speculation-Rules` header's name.
const SPECULATION_RULES: HeaderName = HeaderName::from_static("speculation-rules");

/// What the document head costs before its title and scripts.
const HEAD_BYTES: usize = 512;

/// A whole page: `<head>` from the toolkit, `<body>` from you.
///
/// A page whose tree needs no script ships none: its links are prefetched
/// through the `Speculation-Rules` header, with no JavaScript. A page that
/// needs the runtime loads it, and the runtime boosts its links.
///
/// ```
/// use navree_ui::{View, el};
///
/// let html = View::new("Deals").body(el("main").child("Hello")).to_html();
/// assert!(html.contains("<title>Deals</title>"));
/// assert!(!html.contains("<script"));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct View {
    title: Cow<'static, str>,
    lang: &'static str,
    body: Node,
    status: StatusCode,
    cache: &'static str,
    cookies: Vec<HeaderValue>,
}

impl View {
    /// A page titled `title`.
    pub fn new(title: impl Into<Cow<'static, str>>) -> Self {
        Self {
            title: title.into(),
            lang: "en",
            body: Node::Empty,
            status: StatusCode::OK,
            cache: "no-store",
            cookies: Vec::new(),
        }
    }

    /// Sets the page's language.
    pub fn lang(mut self, lang: &'static str) -> Self {
        self.lang = lang;
        self
    }

    /// Sets the page's content.
    pub fn body(mut self, body: impl IntoNodes) -> Self {
        self.body = Node::list(body);
        self
    }

    /// Sets the status.
    pub fn status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }

    /// Sets `Cache-Control` (`no-store` by default: pages show private data).
    pub fn cache(mut self, cache: &'static str) -> Self {
        self.cache = cache;
        self
    }

    /// Adds a `Set-Cookie` header.
    pub(crate) fn cookie(mut self, cookie: HeaderValue) -> Self {
        self.cookies.push(cookie);
        self
    }

    /// What this page loads.
    pub fn uses(&self) -> Uses {
        measure(&self.body).uses
    }

    /// The page's HTML.
    pub fn to_html(&self) -> String {
        let found = measure(&self.body);
        let mut out = String::with_capacity(found.bytes + self.title.len() + HEAD_BYTES);
        self.write(&mut out, found.uses);
        out
    }

    fn write(&self, out: &mut String, uses: Uses) {
        out.push_str("<!doctype html><html lang=\"");
        out.push_str(self.lang);
        out.push('"');
        if uses.has(Uses::CORE) {
            out.push_str(" data-build=\"");
            out.push_str(assets::BUILD);
            out.push('"');
        }
        out.push_str("><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><meta name=\"color-scheme\" content=\"dark\"><title>");
        escape_into(out, &self.title);
        out.push_str("</title><link rel=\"preload\" as=\"font\" type=\"font/woff2\" crossorigin href=\"");
        out.push_str(assets::GEIST_URL);
        out.push_str("\"><link rel=\"stylesheet\" href=\"");
        out.push_str(assets::NAVREE_UI_URL);
        out.push_str("\"><link rel=\"icon\" href=\"");
        out.push_str(assets::FAVICON_URL);
        out.push_str("\">");
        for url in assets::scripts(uses) {
            out.push_str("<script type=\"module\" src=\"");
            out.push_str(url);
            out.push_str("\"></script>");
        }
        out.push_str(if uses.has(Uses::CORE) { "</head><body data-ui-boost>" } else { "</head><body>" });
        render_into(out, &self.body);
        out.push_str("</body></html>");
    }
}

impl IntoResponse for View {
    fn into_response(self) -> Response {
        let uses = self.uses();
        let html = self.to_html();
        let mut response = (
            self.status,
            [
                (CONTENT_TYPE, HeaderValue::from_static("text/html; charset=utf-8")),
                (CONTENT_SECURITY_POLICY, HeaderValue::from_static(PAGE_CSP)),
                (CACHE_CONTROL, HeaderValue::from_static(self.cache)),
            ],
            html,
        )
            .into_response();
        let headers = response.headers_mut();
        if !uses.has(Uses::CORE) {
            headers.insert(SPECULATION_RULES, HeaderValue::from_static(assets::SPECULATION_HEADER));
        }
        for cookie in self.cookies {
            headers.append(SET_COOKIE, cookie);
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::Cmd;
    use crate::el;

    #[test]
    fn a_static_page_ships_no_script() {
        let html = View::new("x").body(el("main").child("hi")).to_html();
        assert!(!html.contains("<script"), "{html}");
        assert!(!html.contains("data-build"), "{html}");
        assert!(html.contains(assets::NAVREE_UI_URL));
    }

    #[test]
    fn a_command_loads_the_core_runtime() {
        let html = View::new("x").body(el("button").on_click_one(Cmd::toggle("a"))).to_html();
        assert!(html.contains(assets::CORE_URL), "{html}");
        assert!(html.contains("data-ui-boost"), "{html}");
        assert!(html.contains(&format!("data-build=\"{}\"", assets::BUILD)), "{html}");
    }

    #[test]
    fn the_title_is_escaped() {
        let html = View::new("<x>").to_html();
        assert!(html.contains("<title>&lt;x&gt;</title>"), "{html}");
    }

    #[test]
    fn responses_carry_the_policy() {
        let response = View::new("x").into_response();
        assert_eq!(response.headers()[CONTENT_SECURITY_POLICY], PAGE_CSP);
        assert_eq!(response.headers()[CACHE_CONTROL], "no-store");
        assert_eq!(response.headers()["speculation-rules"], assets::SPECULATION_HEADER);
    }

    #[test]
    fn no_inline_code_is_allowed_by_the_policy() {
        assert!(!PAGE_CSP.contains("unsafe"));
    }
}
