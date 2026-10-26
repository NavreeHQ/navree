//! The request extractors of the panel's routes.

use std::sync::Arc;

use axum::extract::FromRequestParts;
use http::header::USER_AGENT;
use http::request::Parts;
use navree_access::CurrentAccess;
use navree_contracts::{AuthState, RequestId};
use navree_db::Uow;
use navree_http::{AppError, ClientIp};
use navree_kernel::AppContext;

use crate::core::Core;
use crate::cx::{Client, Cx};
use crate::session::PanelSession;

/// A signed-in panel request: its [`Cx`] and session.
pub(crate) struct Panel {
    pub(crate) cx: Cx,
    pub(crate) session: PanelSession,
}

impl<S: Send + Sync> FromRequestParts<S> for Panel {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app = parts
            .extensions
            .get::<AppContext>()
            .cloned()
            .ok_or_else(|| AppError::internal("the panel needs the navree request pipeline"))?;
        let core: Arc<Core> = app.get::<Core>().await.map_err(AppError::internal)?;
        let user_id = parts
            .extensions
            .get::<AuthState>()
            .and_then(|state| state.0.as_ref())
            .map(|p| p.user_id)
            .ok_or_else(AppError::unauthenticated)?;
        let uow = Uow::from_request_parts(parts, state).await?;
        let CurrentAccess(access) = CurrentAccess::from_request_parts(parts, state).await?;
        let ClientIp(ip) = ClientIp::from_request_parts(parts, state)
            .await
            .unwrap_or_else(|never| match never {});
        let client = Client {
            request_id: parts
                .extensions
                .get::<RequestId>()
                .map(|id| id.as_str().to_owned()),
            ip: Some(ip.to_string()),
            user_agent: parts
                .headers
                .get(USER_AGENT)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned),
        };
        let session = parts
            .extensions
            .get::<PanelSession>()
            .cloned()
            .unwrap_or_default();
        Ok(Panel {
            cx: Cx {
                app,
                uow,
                access,
                user_id,
                core,
                client,
                tz_offset: None,
            },
            session,
        })
    }
}
