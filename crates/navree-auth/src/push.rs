//! Native apps' push devices and the sign-ins that registered them (feature
//! `notify`): a device belongs to the session it was registered in, so a
//! signed-out phone gets none of its user's notifications.
//!
//! - [`AuthLiveSessions`] is navree-notify's `dyn LiveSessions`: before the
//!   push channel sends, it asks which of the devices' sessions are still
//!   live (not signed out, not ended from another device, not revoked by a
//!   password change, a reset or offboarding, not expired); the devices of
//!   the others are deleted, not sent to. `AuthModule` registers it.
//! - Signing out (`AuthService::sign_out_session`, `logout`), ending a
//!   session (`revoke_session`) and logging out everywhere delete the
//!   devices at once, in the same unit of work.

use std::fmt;
use std::sync::Arc;

use navree_contracts::{BoxError, BoxFuture};
use navree_notify::{LiveSessions, PushDevices, USER_NOTIFIABLE_TYPE};
use uuid::Uuid;

use crate::AuthService;

/// navree-notify's `dyn LiveSessions` answered from navree-auth's sessions
/// (see the module documentation).
pub struct AuthLiveSessions {
    service: Arc<AuthService>,
}

impl AuthLiveSessions {
    /// The live sessions `service` keeps.
    pub fn new(service: Arc<AuthService>) -> Self {
        Self { service }
    }
}

impl LiveSessions for AuthLiveSessions {
    fn live<'a>(
        &'a self,
        user_id: Uuid,
        sessions: &'a [String],
    ) -> BoxFuture<'a, Result<Vec<String>, BoxError>> {
        Box::pin(async move {
            let ids: Vec<Uuid> = sessions
                .iter()
                .filter_map(|session| Uuid::parse_str(session).ok())
                .collect();
            let live = self.service.live_sessions(user_id, &ids).await?;
            Ok(sessions
                .iter()
                .filter(|session| Uuid::parse_str(session).is_ok_and(|id| live.contains(&id)))
                .cloned()
                .collect())
        })
    }
}

impl fmt::Debug for AuthLiveSessions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthLiveSessions").finish_non_exhaustive()
    }
}

/// Which sessions just ended.
pub(crate) enum Ended {
    /// One session (a refresh-token family).
    Session(Uuid),
    /// Every session of the user.
    All,
}

/// Deletes the push devices of the sessions of `user` that just ended, when
/// navree-notify's `PushDevices` is in the app; a failure is logged (the
/// sign-out stands, and the push channel skips those devices anyway).
pub(crate) async fn end_devices(service: &AuthService, user: Uuid, ended: Ended) {
    let Some(app) = service.app.upgrade() else {
        return;
    };
    let Some(devices) = app.try_get::<PushDevices>().await else {
        return;
    };
    let result = match ended {
        Ended::Session(session) => devices.end_session(&session.to_string()).await,
        Ended::All => {
            devices
                .end_all(USER_NOTIFIABLE_TYPE, &[user.to_string()])
                .await
        }
    };
    match result {
        Ok(0) => {}
        Ok(deleted) => tracing::debug!(
            target: "navree::auth",
            user_id = %user,
            deleted,
            "the push devices of an ended sign-in were deleted"
        ),
        Err(error) => tracing::warn!(
            target: "navree::auth",
            user_id = %user,
            %error,
            "could not delete the push devices of an ended sign-in"
        ),
    }
}
