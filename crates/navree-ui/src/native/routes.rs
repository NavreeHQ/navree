//! The route a native client fetches the app's shell from, and the stamps a
//! client keeps its screens under.
//!
//! The shell of a request is built as the error pages build theirs: by the
//! app's [`ErrorShell`](crate::ErrorShell) when it builds its shell per
//! request, else from the one [`Shell`] it registered. So an app whose shell
//! names the person signed in, or counts their unread messages, answers a
//! native client with the same.

use std::hash::{Hash, Hasher};
use std::sync::{Arc, OnceLock};

use http::header::{CACHE_CONTROL, CONTENT_TYPE, VARY};
use http::request::Parts;
use http::{HeaderValue, Method};
use navree_http::response::{IntoResponse, Response};
use navree_http::{ApiMeta, Controller, ErrorContext, RouteDef};
use navree_kernel::{AppContext, BoxFuture, Injectable, KernelError, ProviderKey, Resolver};

use crate::pages::PagesOptions;
use crate::shell::Shell;

/// The version of the reading of pages into native parts: a page that reads
/// into other parts than before is a new number, so a client drops what it kept.
pub const LOWERING: u32 = 1;

/// The app's stamp: what a native client keeps its screens under, and drops
/// them when it changes. It is made of the app's version (`app.version`), the
/// version of the reading of pages and the vocabulary; an app that names no
/// version stamps the time it started, so each start is a new stamp.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamp(Arc<str>);

impl Stamp {
    /// The stamp of an app at `version`.
    ///
    /// ```
    /// use navree_ui::native::Stamp;
    ///
    /// assert_eq!(Stamp::of(Some("1.4.0")), Stamp::of(Some("1.4.0")));
    /// assert_ne!(Stamp::of(Some("1.4.0")), Stamp::of(Some("1.4.1")));
    /// assert_eq!(Stamp::of(Some("1.4.0")).as_str().len(), 12);
    /// ```
    pub fn of(version: Option<&str>) -> Self {
        static STARTED: OnceLock<u128> = OnceLock::new();
        let mut hasher = std::hash::DefaultHasher::new();
        match version {
            Some(version) => version.hash(&mut hasher),
            None => STARTED
                .get_or_init(|| {
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |since| since.as_nanos())
                })
                .hash(&mut hasher),
        }
        LOWERING.hash(&mut hasher);
        navree_native::VOCAB.hash(&mut hasher);
        Self(short(hasher.finish()).into())
    }

    /// The stamp, as it goes on the wire.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Twelve hexadecimal digits of `hash`.
fn short(hash: u64) -> String {
    format!("{:012x}", hash >> 16)
}

/// The app's stamp, from its settings: what the module provides.
pub(crate) async fn app_stamp(ctx: &AppContext) -> Stamp {
    let version = match ctx.try_get::<navree_config::Config>().await {
        Some(config) => config.app_version(),
        None => None,
    };
    Stamp::of(version.as_deref())
}

/// Serves [`SHELL_PATH`](navree_native::SHELL_PATH).
pub(crate) struct Routes {
    ctx: AppContext,
}

impl Injectable for Routes {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(Self {
                ctx: r.app().clone(),
            })
        })
    }
}

/// The shell of the request of `parts`, as an error page's is built: the
/// app's `ErrorShell`, given the person signed in, or the `Shell` it
/// registered for everyone.
async fn shell_of(ctx: &AppContext, parts: &Parts) -> Option<crate::shell::AppShell> {
    let shell = match parts.extensions.get::<Arc<ErrorContext>>() {
        Some(context) => {
            let mut context = context.as_ref().clone();
            context.user = navree_http::CurrentPrincipal::from_parts(parts)
                .ok()
                .map(|principal| principal.0.user_id);
            crate::pages::shell_of(&context).await
        }
        None => ctx.try_get::<Shell>().await.map(|shell| (*shell).clone()),
    }?;
    Some(shell.for_request("/", &parts.headers))
}

/// The app's shell as a native client draws it, for this request: the
/// person signed in and their menu are theirs.
async fn document(ctx: &AppContext, parts: &Parts) -> Response {
    let sign_in = ctx
        .try_get::<PagesOptions>()
        .await
        .map(|options| options.sign_in.clone());
    // The client the document is for: what the app's modules say of it depends on its system.
    let client = match super::scope::read(&parts.headers, "") {
        super::scope::Reading::Asked(super::scope::Asked::Native(request)) => {
            Some(request.client.clone())
        }
        _ => None,
    };
    let contributed = match &client {
        Some(client) => super::swap::shell_parts(ctx, client).await,
        None => crate::NativeShellParts::default(),
    };
    let document = match shell_of(ctx, parts).await {
        Some(shell) => shell.native_document(sign_in.as_deref(), &contributed),
        None => {
            let mut document = navree_native::Shell::new(crate::assets::BUILD, "");
            document.routes = crate::shell::native_routes(sign_in.as_deref());
            crate::shell::native_contributed(&mut document, &contributed);
            document.stamped()
        }
    };
    let body = serde_json::to_string(&document).unwrap_or_else(|_| String::from("{}"));
    (
        [
            (
                CONTENT_TYPE,
                HeaderValue::from_static(navree_native::CONTENT_TYPE),
            ),
            // The user's menu and its token differ from reader to reader.
            (CACHE_CONTROL, HeaderValue::from_static("no-store")),
            (VARY, HeaderValue::from_static("cookie")),
        ],
        body,
    )
        .into_response()
}

impl Controller for Routes {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let ctx = self.ctx.clone();
        vec![
            RouteDef::on(
                Method::GET,
                format!("!{}", navree_native::SHELL_PATH),
                move |parts: Parts| {
                    let ctx = ctx.clone();
                    async move { document(&ctx, &parts).await }
                },
            )
            .name("native.shell")
            .public()
            .api(ApiMeta {
                summary: Some("The app's shell, for a native client".to_owned()),
                description: Some(
                    "The app's name, its destinations, the menu of the person signed in, its \
                     theme's accent and how its screens are presented, as data a native client \
                     draws its own chrome from. Every page drawn in it carries its `stamp` \
                     as `shell`; a client fetches it again when an answer carries another."
                        .to_owned(),
                ),
                tags: vec!["UI".to_owned()],
                ..ApiMeta::default()
            }),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stamp_changes_with_the_app_and_nothing_else() {
        let one = Stamp::of(Some("2.1.0"));
        assert_eq!(one, Stamp::of(Some("2.1.0")));
        assert_ne!(one, Stamp::of(Some("2.1.1")));
        // With no version, each start of the app is its own stamp; within one, it holds.
        assert_eq!(Stamp::of(None), Stamp::of(None));
        assert_ne!(Stamp::of(None), one);
        assert!(one.as_str().bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
}
