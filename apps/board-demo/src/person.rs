//! The one person the board shows to everyone, named to the framework.
//!
//! The board has no sign-in: every visitor works as its one team account. The
//! framework still needs to be told who that is on each request, the way an
//! app with a sign-in tells it, so that what is kept for a person (a native
//! client keeps the place a person was at, for them alone) has a person to
//! keep it for.

use http::request::Parts;
use navree::BoxFuture;
use navree::contracts::{AuthVia, Principal};
use navree::http::{AppError, Authenticator};
use uuid::Uuid;

/// The id of the team account the shell shows, the same on every request and
/// at every start.
pub const TEAM_ID: Uuid = Uuid::from_u128(0x2b7f_93a0_d5c1_4e68_b34a_71e9_0c5d_a826);

/// Names the team account as the person of every request.
///
/// This is a demo's way: the board has no sign-in, so every visitor is its one
/// account. An app with accounts names the person through its sign-in
/// (`navree-auth` registers the `Authenticator` that does it).
pub struct Team;

impl Authenticator for Team {
    fn authenticate<'a>(
        &'a self,
        _parts: &'a mut Parts,
    ) -> BoxFuture<'a, Result<Option<Principal>, AppError>> {
        Box::pin(async {
            Ok(Some(Principal {
                user_id: TEAM_ID,
                via: AuthVia::Session,
                token_abilities: None,
                session_id: None,
            }))
        })
    }
}
