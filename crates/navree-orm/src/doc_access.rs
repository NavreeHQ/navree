//! The model the access-control documentation examples use (`Ticket`, a
//! subject with a policy). Not API: hidden from the docs, and it may change
//! at any time.

pub use chrono::{DateTime, Utc};
pub use navree_db::{Db, Uow};
pub use uuid::Uuid;

pub use crate::prelude::*;
pub use crate::{Result, Value};

use navree_access::{AbilityCtx, Policy, READ, Rules, UPDATE, field};

/// A support ticket: a subject owned by a user.
#[derive(Model, Clone, Debug, Default, serde::Serialize)]
#[model(timestamps, subject, policy = TicketPolicy)]
pub struct Ticket {
    /// The key (a UUID v7).
    pub id: Uuid,
    /// The user who owns the ticket.
    pub owner_id: Option<Uuid>,
    /// The title.
    pub title: String,
    /// `open` or `closed`.
    pub status: String,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

/// Signed-in users read every ticket and update their own.
pub struct TicketPolicy;

impl Policy for TicketPolicy {
    fn define(ctx: &AbilityCtx, rules: &mut Rules) {
        let Some(user) = &ctx.user else {
            return;
        };
        rules.can(READ, "Ticket");
        rules.can(UPDATE, "Ticket").when(field("owner_id").eq(user.id));
    }
}
