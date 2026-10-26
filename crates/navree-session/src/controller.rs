//! [`CsrfCookieController`]: `GET /sanctum/csrf-cookie`.

use std::sync::Arc;

use axum::http::Method;
use navree_http::{ApiMeta, Controller, NoContent, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Resolver};

use crate::Session;

/// Serves `GET /sanctum/csrf-cookie` (absolute: no API prefix), the
/// Sanctum-style handshake of single-page apps: 204 with the `XSRF-TOKEN`
/// cookie (and the session cookie when the session is new).
pub(crate) struct CsrfCookieController;

impl Injectable for CsrfCookieController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(CsrfCookieController) })
    }
}

impl Controller for CsrfCookieController {
    fn prefix(&self) -> &'static str {
        "!/sanctum"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "/csrf-cookie", csrf_cookie)
                .public()
                .name("sanctum.csrf-cookie")
                .api(ApiMeta {
                    summary: Some("Set the XSRF-TOKEN cookie".to_owned()),
                    description: Some(
                        "Starts a session if needed and sets the `XSRF-TOKEN` cookie; send its \
                         URL-decoded value as `X-XSRF-TOKEN` on every state-changing request."
                            .to_owned(),
                    ),
                    tags: vec!["Session".to_owned()],
                    success_status: 204,
                    ..ApiMeta::default()
                }),
        ]
    }
}

async fn csrf_cookie(session: Session) -> NoContent {
    session.csrf_token();
    NoContent
}
