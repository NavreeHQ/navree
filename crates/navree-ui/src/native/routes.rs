//! The route a native client fetches the app's shell from.

use std::sync::Arc;

use http::header::{CACHE_CONTROL, CONTENT_TYPE, VARY};
use http::{HeaderMap, HeaderValue, Method};
use navree_http::response::{IntoResponse, Response};
use navree_http::{ApiMeta, Controller, RouteDef};
use navree_kernel::{AppContext, BoxFuture, Injectable, KernelError, ProviderKey, Resolver};

use crate::assets;
use crate::shell::Shell;

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

/// The app's shell as a native client draws it: read from the shell the app
/// registered, drawn around an empty page for this request. An app with no
/// shell answers a document with no destinations.
async fn document(ctx: &AppContext, headers: &HeaderMap) -> Response {
    let document = match ctx.try_get::<Shell>().await {
        Some(shell) => {
            let view = shell.for_request("/", headers).page("").to_view();
            let mut document = super::page::shell(assets::BUILD, view.body_node())
                .unwrap_or_else(|| navree_native::Shell::new(assets::BUILD, ""));
            document.theme = view
                .look()
                .into_iter()
                .find(|(name, _)| *name == "data-theme")
                .map(|(_, theme)| navree_native::shell::Theme {
                    name: Some(theme),
                    ..navree_native::shell::Theme::default()
                });
            document
        }
        None => navree_native::Shell::new(assets::BUILD, ""),
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
                move |headers: HeaderMap| {
                    let ctx = ctx.clone();
                    async move { document(&ctx, &headers).await }
                },
            )
            .name("native.shell")
            .public()
            .api(ApiMeta {
                summary: Some("The app's shell, for a native client".to_owned()),
                description: Some(
                    "The app's name, its destinations and the menu of the person signed in, as \
                     data a native client draws its own chrome from. It is fetched once, and \
                     again when an answer's build changes."
                        .to_owned(),
                ),
                tags: vec!["UI".to_owned()],
                ..ApiMeta::default()
            }),
        ]
    }
}
