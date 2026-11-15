//! The person each request is, named to the framework.
//!
//! The board has no sign-in: a visitor views it as the member of the team their cookie
//! names (the account menu's "view as"), or as the first member. The framework still needs to
//! be told who that is on each request, the way an app with a sign-in tells it, so that what
//! is kept for a person (a native client keeps the place a person was at, for them alone) has
//! a person to keep it for, and a visitor who views the board as someone else is another
//! person.

use http::request::Parts;
use navree::BoxFuture;
use navree::contracts::{AuthVia, Principal};
use navree::http::{AppError, Authenticator};
use uuid::Uuid;

use crate::data::viewer_number;
use crate::pages::viewer_id;

/// The first half of every viewer's id: what makes them the board's.
const VIEWERS: u128 = 0x2b7f_93a0_d5c1_4e68;

/// The id of the person who views the board as the team member numbered `viewer`: the same at
/// every start.
pub fn person_of(viewer: i64) -> Uuid {
    // The low half is the member's number; the version and variant bits of a UUID are
    // set so it reads as one.
    let low = u128::from(viewer.unsigned_abs()) & 0x0fff_ffff_ffff_ffff;
    Uuid::from_u128((VIEWERS << 64) | 0x8000_0000_0000_0000 | low)
}

/// Names the team member a visitor views the board as the person of their request.
///
/// This is a demo's way: the board has no sign-in, so every visitor is the member they chose
/// to view it as (the first member until they choose). An app with accounts names the person
/// through its sign-in (`navree-auth` registers the `Authenticator` that does it).
pub struct Viewers;

impl Authenticator for Viewers {
    fn authenticate<'a>(
        &'a self,
        parts: &'a mut Parts,
    ) -> BoxFuture<'a, Result<Option<Principal>, AppError>> {
        // A cookie that names nobody on the team shows the board as the first member, as the
        // pages do.
        let viewer = viewer_number(viewer_id(&parts.headers));
        Box::pin(async move {
            Ok(Some(Principal {
                user_id: person_of(viewer),
                via: AuthVia::Session,
                token_abilities: None,
                session_id: None,
            }))
        })
    }
}
