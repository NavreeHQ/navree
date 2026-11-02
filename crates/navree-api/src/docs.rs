//! The routes serving the document: `GET {api_prefix}/openapi.json`, the API
//! reference page at `GET {api_prefix}/docs` and the page's own stylesheet and
//! script at `GET {api_prefix}/docs/assets/{file}`.
//!
//! The page is rendered here from the document, and its stylesheet and script
//! are embedded in the binary, so it loads nothing from a third party and its
//! policy allows styles and scripts from `'self'` only.

use std::sync::Arc;

use http::header::{CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE};
use http::{HeaderValue, Method};
use navree_http::body::Bytes;
use navree_http::response::{IntoResponse, Response};
use navree_http::{AppError, Controller, HttpOptions, Path, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use tokio::sync::OnceCell;

use crate::ApiDocs;
use crate::reference::{Links, render};

/// The stylesheet's and the script's file names. They carry the version, so a
/// response is cached for a year.
const STYLESHEET: &str = concat!("reference-", env!("CARGO_PKG_VERSION"), ".css");
const SCRIPT: &str = concat!("reference-", env!("CARGO_PKG_VERSION"), ".js");

/// One embedded file, served under `{api_prefix}/docs/assets/{path}`.
struct Asset {
    path: &'static str,
    content_type: &'static str,
    bytes: &'static [u8],
}

const ASSETS: &[Asset] = &[
    Asset {
        path: STYLESHEET,
        content_type: "text/css; charset=utf-8",
        bytes: include_bytes!("../assets/reference.css"),
    },
    Asset {
        path: SCRIPT,
        content_type: "text/javascript; charset=utf-8",
        bytes: include_bytes!("../assets/reference.js"),
    },
];

/// The cache policy of the versioned files.
const ASSET_CACHE: &str = "public, max-age=31536000, immutable";

/// The docs page's content security policy: its own stylesheet and script,
/// nothing inline, API calls to this origin. Server URLs from the options are
/// added to `connect-src`.
const PAGE_CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'";

/// The docs routes of `ApiModule` (public, under the API prefix, unversioned).
pub(crate) struct DocsController {
    docs: Arc<ApiDocs>,
    page: Option<Arc<DocsPage>>,
}

/// The docs page, rendered the first time it's asked for, and its policy.
struct DocsPage {
    html: OnceCell<Bytes>,
    csp: HeaderValue,
    /// Where the page links to the document, when it is served.
    json: Option<String>,
    stylesheet: String,
    script: String,
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
                Arc::new(DocsPage {
                    html: OnceCell::new(),
                    csp: page_csp(&docs.options().servers),
                    json: docs
                        .options()
                        .openapi_json
                        .then(|| format!("{prefix}/openapi.json")),
                    stylesheet: format!("{prefix}/docs/assets/{STYLESHEET}"),
                    script: format!("{prefix}/docs/assets/{SCRIPT}"),
                })
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
        let mut routes = Vec::new();
        if self.docs.options().openapi_json {
            let docs = self.docs.clone();
            routes.push(
                RouteDef::on(Method::GET, "/openapi.json", move || {
                    let docs = docs.clone();
                    async move { openapi_json(&docs).await }
                })
                .name("api.openapi")
                .public(),
            );
        }
        if let Some(page) = &self.page {
            let page = page.clone();
            let docs = self.docs.clone();
            routes.push(
                RouteDef::on(Method::GET, "/docs", move || {
                    let page = page.clone();
                    let docs = docs.clone();
                    async move { docs_page(&docs, &page).await }
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

/// `GET {api_prefix}/docs`: the page.
async fn docs_page(docs: &ApiDocs, page: &DocsPage) -> Response {
    let html = page
        .html
        .get_or_try_init(|| async {
            let document = docs.model().await?;
            let html = render(
                document,
                &Links {
                    json: page.json.as_deref(),
                    stylesheet: &page.stylesheet,
                    script: &page.script,
                },
            );
            Ok::<_, navree_kernel::BoxError>(Bytes::from(html))
        })
        .await;
    match html {
        Ok(html) => (
            [
                (
                    CONTENT_TYPE,
                    HeaderValue::from_static("text/html; charset=utf-8"),
                ),
                (CONTENT_SECURITY_POLICY, page.csp.clone()),
            ],
            html.clone(),
        )
            .into_response(),
        Err(error) => AppError::internal(error).into_response(),
    }
}

/// `GET {api_prefix}/docs/assets/{file}`: an embedded file.
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

/// The absolute server URLs safe in a policy: what the "Try it" form can call.
pub(crate) fn callable_servers(servers: &[String]) -> impl Iterator<Item = &String> {
    servers
        .iter()
        .filter(|server| server.starts_with("https://") || server.starts_with("http://"))
        .filter(|server| !server.contains([';', ',', ' ', '\'']))
}

/// [`PAGE_CSP`] with the absolute server URLs (those safe in a policy) added
/// to `connect-src`, so the "Try it" form can call them.
fn page_csp(servers: &[String]) -> HeaderValue {
    let connect: String = callable_servers(servers)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_policy_allows_only_this_origin_for_code_and_style() {
        assert!(PAGE_CSP.contains("script-src 'self';"), "{PAGE_CSP}");
        assert!(PAGE_CSP.contains("style-src 'self';"), "{PAGE_CSP}");
        assert!(PAGE_CSP.contains("default-src 'none';"), "{PAGE_CSP}");
        assert!(!PAGE_CSP.contains("unsafe"), "{PAGE_CSP}");
        assert!(!PAGE_CSP.contains("https:"), "{PAGE_CSP}");
    }

    /// The page loads no image from another origin, so it cannot be made to
    /// fetch one: images come from this origin or `data:` URIs only.
    #[test]
    fn the_page_loads_images_from_its_own_origin_and_data_uris_only() {
        let images = |policy: &str| {
            policy
                .split(';')
                .map(str::trim)
                .find(|directive| directive.starts_with("img-src"))
                .map(str::to_owned)
        };
        assert_eq!(images(PAGE_CSP).as_deref(), Some("img-src 'self' data:"));
        let with_servers = page_csp(&["https://api.example.com".to_owned()]);
        assert_eq!(
            images(with_servers.to_str().unwrap()).as_deref(),
            Some("img-src 'self' data:")
        );
    }

    #[test]
    fn the_embedded_files_are_there_and_carry_their_version() {
        for asset in ASSETS {
            assert!(!asset.bytes.is_empty(), "{}", asset.path);
            assert!(
                asset.path.contains(env!("CARGO_PKG_VERSION")),
                "{}",
                asset.path
            );
        }
    }

    /// Everything the stylesheet and the script ask of the page's policy: no
    /// third-party URL, no HTML built from a string, nothing written to a
    /// `style` attribute.
    #[test]
    fn the_embedded_files_load_nothing_else() {
        for asset in ASSETS {
            let text = std::str::from_utf8(asset.bytes).unwrap();
            assert!(!text.contains("http://"), "{}", asset.path);
            assert!(!text.contains("https://"), "{}", asset.path);
            assert!(!text.contains("@import"), "{}", asset.path);
            assert!(!text.contains("url("), "{}", asset.path);
            assert!(!text.contains(".innerHTML"), "{}", asset.path);
            assert!(!text.contains("insertAdjacentHTML"), "{}", asset.path);
            assert!(!text.contains(".style."), "{}", asset.path);
            assert!(!text.contains("eval("), "{}", asset.path);
        }
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
