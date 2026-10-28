//! The routes serving the document: `GET {api_prefix}/openapi.json`, the
//! Scalar docs UI at `GET {api_prefix}/docs` and the UI's own script at
//! `GET {api_prefix}/docs/assets/{file}`.
//!
//! The script is vendored (embedded in the binary, pinned to one version,
//! with its license file) and served by the app itself, so the page loads
//! nothing from a third party and its policy allows scripts from `'self'`
//! only.

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::Path;
use axum::http::header::{CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE};
use axum::http::{HeaderValue, Method};
use axum::response::{IntoResponse, Response};
use navree_http::{AppError, Controller, HttpOptions, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use utoipa_scalar::Scalar;

use crate::ApiDocs;

/// One vendored file, served under `{api_prefix}/docs/assets/{path}`. The
/// file names carry the version, so a response is cached for a year.
struct Asset {
    path: &'static str,
    content_type: &'static str,
    bytes: &'static [u8],
}

/// The Scalar UI script: the `@scalar/api-reference` 1.74.0 standalone
/// build from npm (`dist/browser/standalone.js`, MIT, unmodified), and its
/// license. Updating means replacing both files and the names here.
const ASSETS: &[Asset] = &[
    Asset {
        path: SCALAR_SCRIPT,
        content_type: "text/javascript; charset=utf-8",
        bytes: include_bytes!("../assets/scalar-api-reference-1.74.0.js"),
    },
    Asset {
        path: "scalar-api-reference-1.74.0.LICENSE.txt",
        content_type: "text/plain; charset=utf-8",
        bytes: include_bytes!("../assets/scalar-api-reference-1.74.0.LICENSE.txt"),
    },
];

/// The vendored script's file name (the version is part of it).
const SCALAR_SCRIPT: &str = "scalar-api-reference-1.74.0.js";

/// The cache policy of the versioned files.
const ASSET_CACHE: &str = "public, max-age=31536000, immutable";

/// The docs page's content security policy: scripts from this origin only
/// (the vendored Scalar script), the inline styles the UI injects, API calls
/// to this origin. Server URLs from the options are added to `connect-src`.
const PAGE_CSP: &str = "default-src 'none'; script-src 'self'; style-src 'unsafe-inline'; font-src data:; img-src 'self' data: https:; connect-src 'self'; worker-src blob:; frame-ancestors 'none'; base-uri 'none'; form-action 'none'";

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
                let prefix = http.api_prefix.trim_end_matches('/');
                let url = format!("{prefix}/openapi.json");
                let script = format!("{prefix}/docs/assets/{SCALAR_SCRIPT}");
                DocsPage {
                    html: Bytes::from(render_page(&docs.options().title, &url, &script)),
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
            routes.push(
                RouteDef::on(
                    Method::GET,
                    "/docs/assets/{file}",
                    |Path(file): Path<String>| async move { serve_asset(&file) },
                )
                .name("api.docs.assets")
                .public(),
            );
        }
        routes
    }
}

/// `GET {api_prefix}/docs/assets/{file}`: a vendored file.
fn serve_asset(file: &str) -> Response {
    let Some(asset) = ASSETS.iter().find(|asset| asset.path == file) else {
        return AppError::not_found().into_response();
    };
    (
        [
            (CONTENT_TYPE, HeaderValue::from_static(asset.content_type)),
            (CACHE_CONTROL, HeaderValue::from_static(ASSET_CACHE)),
        ],
        Bytes::from_static(asset.bytes),
    )
        .into_response()
}

async fn openapi_json(docs: &ApiDocs) -> Response {
    match docs.json().await {
        Ok(json) => ([(CONTENT_TYPE, "application/json")], json).into_response(),
        Err(error) => AppError::internal(error).into_response(),
    }
}

/// The Scalar page: it loads the document from `url` (rather than embedding
/// it), so no document text ends up inside the HTML, and the script from
/// `script`, a path on this origin. The UI's own web fonts (a third-party
/// stylesheet) are off.
fn render_page(title: &str, url: &str, script: &str) -> String {
    let template = format!(
        r#"<!doctype html>
<html>
<head>
    <title>$title</title>
    <meta charset="utf-8"/>
    <meta name="viewport" content="width=device-width, initial-scale=1"/>
</head>
<body>
<script id="api-reference" data-url="{url}" data-configuration='{{"withDefaultFonts":false}}'></script>
<script src="{script}"></script>
</body>
</html>
"#,
        url = escape_html(url),
        script = escape_html(script),
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
        let html = render_page("A & <B>", "/api\"/openapi.json", "/api/docs/assets/x.js");
        assert!(html.contains("<title>A &amp; &lt;B&gt;</title>"), "{html}");
        assert!(
            html.contains(r#"data-url="/api&quot;/openapi.json""#),
            "{html}"
        );
    }

    #[test]
    fn the_page_loads_only_its_own_script() {
        let html = render_page("API", "/api/openapi.json", "/api/docs/assets/s.js");
        assert!(html.contains(r#"<script src="/api/docs/assets/s.js"></script>"#));
        assert!(
            !html.contains("jsdelivr") && !html.contains("unpkg"),
            "{html}"
        );
        assert!(!PAGE_CSP.contains("https://cdn"), "{PAGE_CSP}");
        assert!(PAGE_CSP.contains("script-src 'self';"), "{PAGE_CSP}");
    }

    #[test]
    fn the_vendored_files_are_there_and_carry_their_version() {
        for asset in ASSETS {
            assert!(!asset.bytes.is_empty(), "{}", asset.path);
            assert!(asset.path.contains("1.74.0"), "{}", asset.path);
        }
        assert!(ASSETS.iter().any(|a| a.path == SCALAR_SCRIPT));
        let license = ASSETS
            .iter()
            .find(|a| a.path.ends_with("LICENSE.txt"))
            .unwrap();
        assert!(
            std::str::from_utf8(license.bytes)
                .unwrap()
                .contains("MIT License")
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
