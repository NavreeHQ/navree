//! The request extractors of the panel's routes.

use std::sync::Arc;

use http::header::USER_AGENT;
use http::request::Parts;
use navree_access::{CurrentAccess, PinnedWorkspace, WorkspaceGate};
use navree_contracts::{AuthState, RequestId};
use navree_db::Uow;
use navree_http::extract::FromRequestParts;
use navree_http::{AppError, ClientIp};
use navree_kernel::AppContext;

use crate::core::Core;
use crate::cx::{Client, Cx};
use crate::session::PanelSession;
use crate::workspace::{self, Admitted, NamedBy, RefusedWrite};

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
        let pin = parts.extensions.get::<PinnedWorkspace>().map(|pin| pin.0);
        // What the app entered is held to the credential as well.
        if let (Some(pin), Some(entered)) = (pin, uow.workspace())
            && pin != entered
        {
            return Err(PinnedWorkspace::mismatch());
        }
        let mut named_by = None;
        let mut staff = None;
        let mut refused_write = None;
        // A request that is not inside a workspace may name one, and is let in
        // only if the user may work in it.
        if enter_named && uow.workspace().is_none() {
            let named =
                workspace::requested(&parts.headers, workspace::header_of(&app).await.as_deref());
            let named = match pin {
                Some(pin) => Some(workspace::within_pin(pin, named)?),
                None => named,
            };
            if let Some((named, by)) = named {
                match workspace::admit_request(parts, &app, &core, &uow, user_id, &access, named)
                    .await
                {
                    Ok(Ok(admitted)) => {
                        named_by = Some(by);
                        if admitted == Admitted::Staff {
                            staff = Some(named);
                        }
                    }
                    // A cookie is what the browser kept: it stops counting, and the
                    // user can still reach the form that changes it.
                    Ok(Err(_)) if by == NamedBy::Cookie => {}
                    Ok(Err(refused)) => return Err(refused.error()),
                    // Nor does it get in the way of a write somewhere else: a
                    // write that would have gone there is told why it can't.
                    Err(error) => match RefusedWrite::of(&error) {
                        Some(refused) if by == NamedBy::Cookie => refused_write = Some(refused),
                        _ => return Err(error),
                    },
                }
            }
        }
        // What the workspace the request is in takes from the user, for a request
        // that may write. A workspace the app entered hasn't been asked yet.
        let mut writes = None;
        if enter_named
            && !matches!(
                parts.method,
                http::Method::GET
                    | http::Method::HEAD
                    | http::Method::OPTIONS
                    | http::Method::TRACE
            )
            && let Some(entered) = uow.workspace()
            && let Some(gate) = app.try_get::<dyn WorkspaceGate>().await
        {
            let allowed = gate.writes(user_id, entered).await?;
            if named_by.is_none()
                && let Some(refusal) = allowed.refusal()
            {
                return Err(refusal);
            }
            writes = Some(allowed);
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
            pin,
            writes,
            refused_write,
        };
        if let Some(entered) = staff {
            cx.audit_outside(workspace::staff_entry(
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
