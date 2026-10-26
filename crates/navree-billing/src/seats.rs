//! [`SeatSync`]: seats follow a workspace's member count.

use navree_events::Listener;
use navree_kernel::{
    AppContext, BoxError, BoxFuture, Injectable, KernelError, Resolver, WeakAppContext,
};
use navree_workspace::events::{MemberAdded, MemberRemoved};
use uuid::Uuid;

use crate::billing::Billing;

/// Listens to the workspace events `MemberAdded` and `MemberRemoved` and
/// updates the quantity of the workspace's subscription when its plan has
/// `seats = true` ([`Billing::sync_seats`]). Queued: it runs on the queue
/// when the app has one, inline otherwise.
pub struct SeatSync {
    app: WeakAppContext,
}

impl Injectable for SeatSync {
    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(SeatSync {
                app: r.app().downgrade(),
            })
        })
    }
}

impl SeatSync {
    async fn sync(&self, workspace_id: Uuid) -> Result<(), BoxError> {
        let Some(app) = self.app.upgrade() else {
            return Ok(());
        };
        let billing = app.get::<Billing>().await?;
        billing.sync_seats(workspace_id).await?;
        Ok(())
    }
}

impl Listener<MemberAdded> for SeatSync {
    fn handle<'a>(
        &'a self,
        event: &'a MemberAdded,
        _ctx: &'a AppContext,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(self.sync(event.workspace_id))
    }

    fn queued(&self) -> bool {
        true
    }
}

impl Listener<MemberRemoved> for SeatSync {
    fn handle<'a>(
        &'a self,
        event: &'a MemberRemoved,
        _ctx: &'a AppContext,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(self.sync(event.workspace_id))
    }

    fn queued(&self) -> bool {
        true
    }
}
