//! The person each request is, named to the framework.
//!
//! The inbox has no sign-in: a visitor works as the agent their signed cookie
//! names, or as the default one. The framework still needs to be told who that
//! is on each request, the way an app with a sign-in tells it, so that what is
//! kept for a person (a native client keeps the place a person was at, for
//! them alone) has a person to keep it for, and a visitor who switches to
//! another agent is another person.

use std::sync::Arc;

use chrono::Utc;
use http::request::Parts;
use navree::BoxFuture;
use navree::contracts::{AuthVia, Principal};
use navree::http::{AppError, Authenticator, UrlSigner};
use uuid::Uuid;

use crate::data::DEFAULT_AGENT;
use crate::pages::{AGENT_COOKIE, agent_of_cookie, cookie};

/// The first half of every agent's id: what makes them the inbox's.
const AGENTS: u128 = 0x7c31_e9a2_5b84_4d06;

/// The id of the person who works as `agent`: the same at every start.
pub fn person_of(agent: i64) -> Uuid {
    // The low half is the agent's number; the version and variant bits of a
    // UUID are set so it reads as one.
    let low = u128::from(agent.unsigned_abs()) & 0x0fff_ffff_ffff_ffff;
    Uuid::from_u128((AGENTS << 64) | 0x8000_0000_0000_0000 | low)
}

/// Names the agent a visitor works as the person of their request.
///
/// This is a demo's way: the inbox has no sign-in, so every visitor is the
/// agent they chose (the default one until they choose). An app with accounts
/// names the person through its sign-in (`navree-auth` registers the
/// `Authenticator` that does it).
pub struct Agents {
    signer: Arc<UrlSigner>,
}

impl Agents {
    /// The authenticator that reads the agent from the cookie `signer` signed.
    pub fn new(signer: Arc<UrlSigner>) -> Self {
        Self { signer }
    }
}

impl Authenticator for Agents {
    fn authenticate<'a>(
        &'a self,
        parts: &'a mut Parts,
    ) -> BoxFuture<'a, Result<Option<Principal>, AppError>> {
        // An unsigned, forged or expired cookie names nobody: the default agent.
        let agent = cookie(&parts.headers, AGENT_COOKIE)
            .and_then(|value| agent_of_cookie(&self.signer, value, Utc::now()))
            .unwrap_or(DEFAULT_AGENT);
        Box::pin(async move {
            Ok(Some(Principal {
                user_id: person_of(agent),
                via: AuthVia::Session,
                token_abilities: None,
                session_id: None,
            }))
        })
    }
}
