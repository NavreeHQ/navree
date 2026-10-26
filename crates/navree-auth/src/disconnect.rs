//! Ending real-time subscriptions when a user signs out (PRD "Where rules
//! are enforced" #5): their sockets close through
//! `dyn Broadcaster::end_user_subscriptions` (navree-realtime: code 4200), so
//! every private channel must be authorized again with valid credentials.

use navree_contracts::Broadcaster;
use navree_db::Uow;
use navree_kernel::WeakAppContext;
use uuid::Uuid;

/// Ends `user`'s real-time subscriptions once the task's unit of work
/// commits (the request's, when the sign-out runs in one), or now when
/// there is none. Does nothing without a `dyn Broadcaster`; a failure is
/// logged (the sign-out itself stands).
pub(crate) async fn end_user_subscriptions(app: &WeakAppContext, user: Uuid) {
    let Some(ctx) = app.upgrade() else {
        return;
    };
    let Some(broadcaster) = ctx.try_get::<dyn Broadcaster>().await else {
        return;
    };
    let end = async move {
        if let Err(error) = broadcaster.end_user_subscriptions(user).await {
            tracing::warn!(
                target: "navree::auth",
                %error,
                user_id = %user,
                "could not end the user's real-time subscriptions after a sign-out"
            );
        }
    };
    match Uow::current() {
        Some(uow) if !uow.is_finished().await => uow.after_commit(move || Box::pin(end)),
        _ => end.await,
    }
}
