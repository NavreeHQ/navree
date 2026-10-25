//! The routes serving the document: `GET {api_prefix}/openapi.json` and the
//! Scalar docs UI at `GET {api_prefix}/docs`.

use std::sync::Arc;

use axum::body::Bytes;
use axum::http::header::{CONTENT_SECURITY_POLICY, CONTENT_TYPE};
use axum::http::{HeaderValue, Method};
use axum::response::{IntoResponse, Response};
use keel_http::{AppError, Controller, HttpOptions, RouteDef};
use keel_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use utoipa_scalar::Scalar;

use crate::ApiDocs;

/// The Scalar UI script (major version 1, from the CDN the page's CSP allows).
const SCALAR_SCRIPT: &str = "https://cdn.jsdelivr.net/npm/@scalar/api-reference@1";

/// The docs page's content security policy: the Scalar script from its CDN,
/// the inline styles and fonts it injects, API calls to this origin. Server
/// URLs from the options are added to `connect-src`.
const PAGE_CSP: &str = "default-src 'none'; script-src https://cdn.jsdelivr.net; style-src 'unsafe-inline' https://cdn.jsdelivr.net https://fonts.scalar.com; font-src data: https://cdn.jsdelivr.net https://fonts.scalar.com; img-src 'self' data: https:; connect-src 'self'; worker-src blob:; frame-ancestors 'none'; base-uri 'none'; form-action 'none'";

/// The docs routes of `ApiModule` (public, under the API prefix, unversioned).
pub(crate) struct DocsController {
    docs: Arc<ApiDocs>,
    page: Option<DocsPage>,
}

/// The rendered docs page and its content security policy.
#[derive(Clone)]
struct DocsPage {
    html: Bytes,
    csp: HeaderValue,
}

impl Injectable for DocsController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<ApiDocs>(),
            ProviderKey::of::<HttpOptions>(),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            let docs = r.get::<ApiDocs>().await?;
            let http = r.get::<HttpOptions>().await?;
            let page = docs.options().docs_ui.then(|| {
                let url = format!("{}/openapi.json", http.api_prefix.trim_end_matches('/'));
                DocsPage {
                    html: Bytes::from(render_page(&docs.options().title, &url)),
                    csp: page_csp(&docs.options().servers),
                }
            });
            Ok(Self { docs, page })
        })
    }
}

impl Controller for DocsController {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let docs = self.docs.clone();
        let mut routes = vec![
            RouteDef::on(Method::GET, "/openapi.json", move || {
                let docs = docs.clone();
                async move { openapi_json(&docs).await }
            })
            .name("api.openapi")
            .public(),
        ];
        if let Some(page) = &self.page {
            let page = page.clone();
            routes.push(
                RouteDef::on(Method::GET, "/docs", move || {
                    let page = page.clone();
                    async move {
                        (
                            [
                                (
                                    CONTENT_TYPE,
                                    HeaderValue::from_static("text/html; charset=utf-8"),
                                ),
                                (CONTENT_SECURITY_POLICY, page.csp),
                            ],
                            page.html,
                        )
                    }
                })
                .name("api.docs")
                .public(),
            );
        }
        routes
    }
}

async fn openapi_json(docs: &ApiDocs) -> Response {
    match docs.json().await {
        Ok(json) => ([(CONTENT_TYPE, "application/json")], json).into_response(),
        Err(error) => AppError::internal(error).into_response(),
    }
}

/// The Scalar page: it loads the document from `url` (rather than embedding
/// it), so no document text ends up inside the HTML.
fn render_page(title: &str, url: &str) -> String {
    let template = format!(
        r#"<!doctype html>
<html>
<head>
    <title>$title</title>
    <meta charset="utf-8"/>
    <meta name="viewport" content="width=device-width, initial-scale=1"/>
</head>
<body>
<script id="api-reference" data-url="{url}"></script>
<script src="{SCALAR_SCRIPT}"></script>
</body>
</html>
"#,
        url = escape_html(url),
    );
    Scalar::new(serde_json::Value::Null)
        .custom_html(template)
        .title(escape_html(title))
        .to_html()
}

/// [`PAGE_CSP`] with the absolute server URLs (those safe in a policy) added
/// to `connect-src`, so the UI's "try it" client can call them.
fn page_csp(servers: &[String]) -> HeaderValue {
    let connect: String = servers
        .iter()
        .filter(|server| server.starts_with("https://") || server.starts_with("http://"))
        .filter(|server| !server.contains([';', ',', ' ', '\'']))
        .map(|server| format!(" {server}"))
        .collect();
    if connect.is_empty() {
        return HeaderValue::from_static(PAGE_CSP);
    }
    let policy = PAGE_CSP.replace(
        "connect-src 'self'",
        &format!("connect-src 'self'{connect}"),
    );
    HeaderValue::from_str(&policy).unwrap_or(HeaderValue::from_static(PAGE_CSP))
}

/// Escapes text for HTML element content and quoted attribute values.
fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_escapes_title_and_url() {
        let html = render_page("A & <B>", "/api\"/openapi.json");
        assert!(html.contains("<title>A &amp; &lt;B&gt;</title>"), "{html}");
        assert!(
            html.contains(r#"data-url="/api&quot;/openapi.json""#),
            "{html}"
        );
    }

    #[test]
    fn csp_allows_configured_servers_only_when_well_formed() {
        let csp = page_csp(&[
            "https://api.example.com".to_owned(),
            "/relative".to_owned(),
            "https://evil.example; script-src *".to_owned(),
        ]);
        let csp = csp.to_str().unwrap();
        assert!(
            csp.contains("connect-src 'self' https://api.example.com;"),
            "{csp}"
        );
        assert!(!csp.contains("evil"), "{csp}");
        assert_eq!(page_csp(&[]).to_str().unwrap(), PAGE_CSP);
    }
}
