//! The request extractors of the panel's routes.

use std::sync::Arc;

use http::header::USER_AGENT;
use http::request::Parts;
use navree_access::CurrentAccess;
use navree_contracts::{AuthState, RequestId};
use navree_db::Uow;
use navree_http::extract::FromRequestParts;
use navree_http::{AppError, ClientIp};
use navree_kernel::AppContext;

use crate::core::Core;
use crate::cx::{Client, Cx};
use crate::session::PanelSession;
use crate::workspace::{self, Admitted, NamedBy};

/// A signed-in panel request: its [`Cx`] and session.
pub(crate) struct Panel {
    pub(crate) cx: Cx,
    pub(crate) session: PanelSession,
}

/// A [`Panel`] that doesn't enter the workspace the request names, for the
/// form that changes it: the user can leave a workspace they may no longer
/// work in or write to.
pub(crate) struct PanelOutside(pub(crate) Panel);

impl FromRequestParts for PanelOutside {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        Ok(PanelOutside(panel(parts, false).await?))
    }
}

impl FromRequestParts for Panel {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        panel(parts, true).await
    }
}

async fn panel(parts: &mut Parts, enter_named: bool) -> Result<Panel, AppError> {
    {
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
        let uow = Uow::from_request_parts(parts).await?;
        // The user's app-wide access: entering a workspace below doesn't change it.
        let CurrentAccess(access) = CurrentAccess::from_request_parts(parts).await?;
        let mut named_by = None;
        let mut staff = None;
        // A request that is not inside a workspace may name one, and is let in
        // only if the user may work in it.
        if enter_named
            && uow.workspace().is_none()
            && let Some((named, by)) =
                workspace::requested(&parts.headers, workspace::header_of(&app).await.as_deref())
        {
            match workspace::admit_request(parts, &app, &core, &uow, user_id, &access, named)
                .await?
            {
                Ok(admitted) => {
                    named_by = Some(by);
                    if admitted == Admitted::Staff {
                        staff = Some(named);
                    }
                }
                Err(refused) if by == NamedBy::Header => return Err(refused.error()),
                // A cookie is what the browser kept: it stops counting, and the
                // user can still reach the form that changes it.
                Err(_) => {}
            }
        }
        let ClientIp(ip) = ClientIp::from_parts(parts);
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
        let cx = Cx {
            app,
            uow,
            access,
            user_id,
            core,
            client,
            tz_offset: None,
            named_by,
        };
        if let Some(entered) = staff {
            cx.audit(workspace::staff_entry(
                entered,
                workspace::named_by_name(named_by),
            ))
            .await;
        }
        Ok(Panel { cx, session })
    }
}

#[cfg(test)]
mod tests {
    use navree_http::handler::Endpoint;

    use super::*;

    #[test]
    fn handlers_on_the_panel_are_native() {
        async fn handler(_: Panel) {}
        let _ = Endpoint::new(handler);
    }
}
