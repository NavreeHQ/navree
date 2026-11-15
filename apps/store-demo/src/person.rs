//! The one person the shop shows to everyone, named to the framework.
//!
//! The shop has no sign-in: every visitor works as its one staff account. The
//! framework still needs to be told who that is on each request, the way an
//! app with a sign-in tells it, so that what is kept for a person (a native
//! client keeps the place a person was at, for them alone) has a person to
//! keep it for.

use http::request::Parts;
use navree::BoxFuture;
use navree::contracts::{AuthVia, Principal};
use navree::http::{AppError, Authenticator};
use uuid::Uuid;

/// The id of the staff account the shell shows, the same on every request and
/// at every start.
pub const STAFF_ID: Uuid = Uuid::from_u128(0x5d8e_41c6_a0b3_4f27_9e15_3c7a_82d4_b6f1);

/// Names the staff account as the person of every request.
///
/// This is a demo's way: the shop has no sign-in, so every visitor is its one
/// account. An app with accounts names the person through its sign-in
/// (`navree-auth` registers the `Authenticator` that does it).
pub struct Staff;

impl Authenticator for Staff {
    fn authenticate<'a>(
        &'a self,
        _parts: &'a mut Parts,
    ) -> BoxFuture<'a, Result<Option<Principal>, AppError>> {
        Box::pin(async {
            Ok(Some(Principal {
                user_id: STAFF_ID,
                via: AuthVia::Session,
                token_abilities: None,
                session_id: None,
            }))
        })
    }
}
